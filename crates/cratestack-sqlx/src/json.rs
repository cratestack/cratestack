//! Postgres `jsonb` (de)serialization for schema-declared `Json` columns
//! (cratestack#162).
//!
//! `sqlx::types::Json<T>` — what generated model structs used before this
//! fix — round-trips `T` through `T`'s own `Serialize`/`Deserialize`. At
//! the time of cratestack#162, `T = cratestack_core::Value` derived an
//! externally-tagged `Serialize`/`Deserialize` (serde's default for a
//! data-carrying enum), so a column ended up holding `{"Map": {}}` instead
//! of `{}`, `{"List": [...]}` instead of `[...]`, and so on. That broke
//! reading any jsonb cratestack didn't write itself (legacy rows, other
//! writers, manual inserts — they hold *plain* JSON) and broke native
//! jsonb operator queries (`->`/`->>`) against the column, since the real
//! value sat nested under a variant tag.
//!
//! [`Json`] is a from-scratch local newtype (not a re-export of
//! `sqlx::types::Json`, and not `cratestack_core::Json` either — both are
//! foreign types here, so implementing `sqlx::Type`/`Encode`/`Decode` for
//! either would violate Rust's orphan rules) whose Postgres impls convert
//! through [`cratestack_core::Value::to_plain_json`] /
//! [`cratestack_core::Value::from_plain_json`] instead: the untagged,
//! natural JSON shape. As of cratestack#506, `Value`'s own hand-written
//! `Serialize`/`Deserialize` (in `cratestack_core::value::codec`) is
//! *also* untagged on every wire, not just this jsonb column path — see
//! that module's doc for why.
//!
//! The actual jsonb wire format (the leading version byte on binary-format
//! values, `JSON` vs `JSONB` OID dispatch) is delegated to
//! `sqlx::types::Json<serde_json::Value>`, which already gets this right —
//! this module only owns the `Value <-> serde_json::Value` conversion.

use cratestack_core::Value;
use serde::{Deserialize, Serialize};

use crate::sqlx::encode::IsNull;
use crate::sqlx::error::BoxDynError;
use crate::sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef, Postgres};
use crate::sqlx::types::Json as SqlxJson;
use crate::sqlx::{Decode, Encode, Type};

/// Wrapper for a schema-declared `Json` column's Rust field type. See the
/// module docs for why this exists instead of `sqlx::types::Json<Value>`.
///
/// `Serialize`/`Deserialize` stay `#[serde(transparent)]` — delegating
/// straight to `T`'s own impl (for `T = Value`, untagged since
/// cratestack#506) — so the model struct's HTTP/RPC wire representation is
/// unchanged from before this fix. The *jsonb column* codec below
/// (`sqlx::Type` / `Encode` / `Decode`, used for the Postgres bind/
/// row-decode path, never for the wire format) is untagged too; that's
/// the actual cratestack#162 fix, scoped to on-disk storage and
/// independent of `Value`'s own serde impls.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Json(value)
    }
}

impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl Type<Postgres> for Json<Value> {
    fn type_info() -> PgTypeInfo {
        <SqlxJson<serde_json::Value> as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <SqlxJson<serde_json::Value> as Type<Postgres>>::compatible(ty)
    }
}

impl PgHasArrayType for Json<Value> {
    fn array_type_info() -> PgTypeInfo {
        <SqlxJson<serde_json::Value> as PgHasArrayType>::array_type_info()
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        <SqlxJson<serde_json::Value> as PgHasArrayType>::array_compatible(ty)
    }
}

impl<'q> Encode<'q, Postgres> for Json<Value> {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        SqlxJson(self.0.to_plain_json()).encode_by_ref(buf)
    }
}

impl<'r> Decode<'r, Postgres> for Json<Value> {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let SqlxJson(plain) = <SqlxJson<serde_json::Value> as Decode<'r, Postgres>>::decode(value)?;
        Ok(Json(Value::from_plain_json(plain)))
    }
}
