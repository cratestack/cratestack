//! Built-in support for the `FindMany<Model>` procedure-argument type
//! (`.cstack` syntax) — search-with-filters for procedures. Composes
//! with `PageInput` rather than absorbing it — a procedure wanting both
//! filtering and pagination declares two arguments, e.g. `procedure
//! search(query: FindMany<Post>, page: PageInput): Page<Post>`.
//!
//! This module holds only the one piece that's genuinely shared across
//! every model: the per-field operator envelope. Everything else — which
//! fields a model has, which operators apply to which field — is
//! per-model and lives in `cratestack-macros`-generated code
//! (`<Model>Where`, `<Model>SortField`, `<Model>OrderByClause`,
//! `<Model>FindManyInput`), mirroring how `Create<Model>Input`/
//! `Update<Model>Input` are per-model generated structs rather than one
//! shared generic wrapper.

use serde::{Deserialize, Serialize};

/// Every operator a filterable field might support, as one flat
/// optional-per-operator envelope — generated per-model code reads only
/// the operators that make sense for a given field's type (e.g. a
/// `Boolean` field's generated `to_filters()` never looks at `contains`).
/// `V` is the field's own scalar Rust type (`String`, `i64`, `bool`,
/// `chrono::DateTime<Utc>`, ...) — never `Option<V>` even for an optional
/// field, since these operators describe a *value to compare against*,
/// not the field's own nullability (which `is_null` covers instead).
///
/// Deliberately not full parity with every `FieldRef` method:
/// `isTrue`/`isFalse` are omitted (redundant with `eq: true`/`eq: false`
/// once callers have a real JSON boolean) and `eqOrNull` is omitted (a
/// Rust-ergonomics convenience over `eq` + `isNull`, not new capability).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FieldFilterInput<V> {
    pub eq: Option<V>,
    pub ne: Option<V>,
    #[serde(rename = "in")]
    pub in_: Option<Vec<V>>,
    pub lt: Option<V>,
    pub lte: Option<V>,
    pub gt: Option<V>,
    pub gte: Option<V>,
    /// String/`Cuid`/`Uuid` fields only.
    pub contains: Option<String>,
    /// String/`Cuid`/`Uuid` fields only.
    pub starts_with: Option<String>,
    /// Optional-arity fields only. `Some(true)` filters to rows where
    /// the column is `NULL`; `Some(false)` filters to rows where it is
    /// not.
    pub is_null: Option<bool>,
}
