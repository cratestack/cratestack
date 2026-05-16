use crate::{IntoSqlValue, values::FilterValue};

use super::expr::FilterExpr;
use super::op::FilterOp;

/// JSON / JSONB filter predicates. Two flavors:
///
/// * `HasKey` — `col ? 'key'` on PG (key-exists operator). On SQLite
///   this lowers to `json_extract(col, '$.key') IS NOT NULL`, which
///   has the same matches-some-non-null-value semantics for the most
///   common case (records where the schema sometimes carries a key,
///   sometimes doesn't); JSON values explicitly stored as `null`
///   diverge between backends, mirroring the operators themselves.
/// * `GetText` — `col ->> 'key' <op> $1` on PG (extract-as-text +
///   compare). On SQLite the same `json_extract` path with a column
///   accessor handles it. Supported comparison ops are the standard
///   `Eq/Ne/Lt/Lte/Gt/Gte` plus `IsNull` / `IsNotNull`.
///
/// Keys are owned `String` so callers can pass runtime-supplied
/// metric / setting names (e.g. user-driven `model_run_timeseries`
/// queries that pivot on `args.metric`). The column slot stays
/// `&'static str` because columns are always schema-rooted.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonFilter {
    HasKey {
        column: &'static str,
        key: String,
    },
    GetText {
        column: &'static str,
        key: String,
        op: FilterOp,
        value: FilterValue,
    },
}

/// Left-hand operand of a `json_get_text` filter — chain a comparison
/// method (`.eq`, `.lt`, `.is_null`, ...) to produce a [`FilterExpr`].
#[derive(Debug, Clone)]
pub struct JsonTextPath {
    pub(super) column: &'static str,
    pub(super) key: String,
}

impl JsonTextPath {
    pub(super) fn new(column: &'static str, key: String) -> Self {
        Self { column, key }
    }

    fn binary<V: IntoSqlValue>(self, op: FilterOp, value: V) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        })
    }

    pub fn eq<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Eq, value)
    }
    pub fn ne<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Ne, value)
    }
    pub fn lt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Lt, value)
    }
    pub fn lte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Lte, value)
    }
    pub fn gt<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Gt, value)
    }
    pub fn gte<V: IntoSqlValue>(self, value: V) -> FilterExpr {
        self.binary(FilterOp::Gte, value)
    }

    /// `col ->> 'key' IS NULL` — the JSON document either lacks the
    /// key, or stores it as JSON null. (PG and SQLite agree here.)
    pub fn is_null(self) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op: FilterOp::IsNull,
            value: FilterValue::None,
        })
    }

    /// `col ->> 'key' IS NOT NULL` — the JSON document has the key
    /// with a non-null primitive value. Note: a PG `?` test (use
    /// [`super::field_ref::FieldRef::json_has_key`]) treats JSON null
    /// as a present key where this method does not.
    pub fn is_not_null(self) -> FilterExpr {
        FilterExpr::Json(JsonFilter::GetText {
            column: self.column,
            key: self.key,
            op: FilterOp::IsNotNull,
            value: FilterValue::None,
        })
    }
}
