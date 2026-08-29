//! `SqlValue::Decimal`/`NullDecimal` → `push_bind` boundary (cratestack#505
//! Direction 2). Split out of `values.rs` to keep both files under this
//! repo's ~200-LoC convention.
//!
//! `SqlValue::Decimal` holds a `Box<dyn DecimalLike>`, not a fixed concrete
//! type, so this boundary has to downcast to whichever concrete backend(s)
//! this crate's own `decimal-*` Cargo features enabled before it can call
//! `push_bind` — sqlx binds a concrete, `Encode`-implementing type, not a
//! trait object.

use crate::sqlx;

/// Downcasts a boxed [`cratestack_sql::DecimalLike`] to whichever concrete
/// backend(s) this crate's own `decimal-*` Cargo features enabled, then
/// binds that concrete, `Encode`-implementing type. At most one arm below
/// is reachable per bound value — `value` came from exactly one concrete
/// type — but multiple arms may be *compiled in* at once when both
/// features are active; the downcast picks the one that actually matches.
pub(super) fn bind_decimal(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    value: &dyn cratestack_sql::DecimalLike,
) {
    #[cfg(feature = "decimal-rust-decimal")]
    if let Some(value) = value.as_any().downcast_ref::<rust_decimal::Decimal>() {
        query.push_bind(*value);
        return;
    }
    #[cfg(feature = "decimal-bigdecimal")]
    if let Some(value) = value.as_any().downcast_ref::<bigdecimal::BigDecimal>() {
        query.push_bind(value.clone());
        return;
    }
    #[cfg(not(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal")))]
    let _ = (&query, value);
    unreachable!(
        "SqlValue::Decimal held a concrete type not enabled by any decimal-* \
         Cargo feature on cratestack-sqlx — an upstream invariant violation, \
         since cratestack-macros can only ever construct one via a decimal \
         backend it was told about"
    );
}

/// `NullDecimal` carries no payload to downcast, so this just needs *a*
/// concrete `Option<T>::None` whose `T` has `Encode`/`Type` impls against
/// Postgres `NUMERIC` — either backend works identically for a NULL bind
/// (there is no value to lose precision on). Three separate cfg'd
/// definitions (rather than one function with cfg'd `return`s) so each
/// compiled variant has one straight-line tail expression — prefers
/// `decimal-rust-decimal` when both are active, falling back to
/// `decimal-bigdecimal`, matching `bind_decimal`'s own preference order.
#[cfg(feature = "decimal-rust-decimal")]
pub(super) fn bind_null_decimal(query: &mut sqlx::QueryBuilder<sqlx::Postgres>) {
    query.push_bind(Option::<rust_decimal::Decimal>::None);
}

#[cfg(all(feature = "decimal-bigdecimal", not(feature = "decimal-rust-decimal")))]
pub(super) fn bind_null_decimal(query: &mut sqlx::QueryBuilder<sqlx::Postgres>) {
    query.push_bind(Option::<bigdecimal::BigDecimal>::None);
}

// `NullDecimal` itself stays an unconditional `SqlValue` variant (it
// carries no `Decimal` payload), but binding it needs *some* concrete
// `Decimal` type to name a concrete `Option<T>`. Reaching this arm with no
// decimal backend selected on this crate means an `SqlValue::NullDecimal`
// was constructed without going through cratestack-macros' generated
// code, which can't exist for a schema with a `Decimal` field unless a
// backend is selected end-to-end (the same invariant the `pgvector`-off
// arm in `values.rs`'s `push_bind_value` already relies on for
// `Vector`/`NullVector`) — an upstream invariant violation, not a case to
// handle gracefully.
#[cfg(not(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal")))]
pub(super) fn bind_null_decimal(query: &mut sqlx::QueryBuilder<sqlx::Postgres>) {
    let _ = query;
    unreachable!(
        "SqlValue::NullDecimal requires a decimal backend Cargo feature \
         (decimal-rust-decimal or decimal-bigdecimal) on cratestack-sqlx"
    );
}
