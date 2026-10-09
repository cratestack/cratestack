//! Compatibility shim that exposes a `sqlx`-shaped API by re-exporting from
//! `sqlx-core` + `sqlx-postgres` directly.
//!
//! **Why this shim exists:** depending on the `sqlx` umbrella crate transitively
//! pulls `sqlx-sqlite` into the resolve graph (Cargo's resolver materialises the
//! optional dep even when no feature activates it), which pins `libsqlite3-sys
//! ^0.30.1` and conflicts with `rusqlite 0.40`'s `libsqlite3-sys ^0.38` via the
//! `links = "sqlite3"` rule. Going direct to the split crates side-steps the
//! leak entirely. Downstream users keep writing `cratestack::sqlx::X` paths;
//! macro emissions stay unchanged.
//!
//! **SemVer caveat:** `sqlx-core` documents itself as "not meant for general use"
//! without SemVer guarantees. The surface re-exported here is the narrow subset
//! the umbrella `sqlx` crate exposes, which was stable in practice across `0.8.x`
//! and is now pinned at `=0.9.0`. That design paid off at the 0.8→0.9 boundary:
//! upstream's `SqlSafeStr` bound and the `QueryBuilder` lifetime removal landed
//! as *additions to this list* (`AssertSqlSafe`/`SqlSafeStr`/`SqlStr`) plus
//! mechanical call-site edits, with no downstream `::cratestack::sqlx::…` path
//! changing. Treat any future minor the same way: adapt here first.

pub use sqlx_core::Either;
pub use sqlx_core::acquire::Acquire;
pub use sqlx_core::arguments::{Arguments, IntoArguments};
pub use sqlx_core::column::{Column, ColumnIndex};
pub use sqlx_core::connection::{ConnectOptions, Connection};
pub use sqlx_core::database::{self, Database};
pub use sqlx_core::describe::Describe;
pub use sqlx_core::executor::{Execute, Executor};
pub use sqlx_core::from_row::FromRow;
pub use sqlx_core::pool::{self, Pool};
pub use sqlx_core::query::{query, query_with};
pub use sqlx_core::query_as::{query_as, query_as_with};
pub use sqlx_core::query_builder::{self, QueryBuilder};
pub use sqlx_core::query_scalar::{query_scalar, query_scalar_with};
pub use sqlx_core::raw_sql::{RawSql, raw_sql};
pub use sqlx_core::row::Row;
// sqlx 0.9.0 (#3723) narrowed every `query*()`/`raw_sql()` entry point to
// `impl SqlSafeStr`, implemented only for `&'static str` and the
// `AssertSqlSafe` wrapper. Re-exported here rather than left to
// `sqlx_core::sql_str::…` paths so the shim stays the single place that
// knows which upstream module these live in — the same reason every other
// item above is re-exported by name.
pub use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr, SqlStr};
pub use sqlx_core::statement::Statement;
pub use sqlx_core::transaction::{Transaction, TransactionManager};
pub use sqlx_core::type_info::TypeInfo;
pub use sqlx_core::value::{Value, ValueRef};

pub use sqlx_core::error::{self, Error, Result};

#[cfg(feature = "decimal-rust-decimal")]
pub use sqlx_core::migrate;
#[cfg(not(feature = "decimal-rust-decimal"))]
pub use sqlx_core::migrate;

pub use sqlx_postgres::{
    self as postgres, PgConnection, PgExecutor, PgPool, PgTransaction, Postgres,
};

pub mod types {
    pub use sqlx_core::types::*;
}

pub mod encode {
    pub use sqlx_core::encode::{Encode, IsNull};
}
pub use self::encode::Encode;

pub mod decode {
    pub use sqlx_core::decode::Decode;
}
pub use self::decode::Decode;

pub use sqlx_core::types::Type;
