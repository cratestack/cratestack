//! CrateStack embedded facade — rusqlite + shared schema surface.
//!
//! This crate is the embedded slice of the framework. It re-exports the
//! shared schema / parser / policy / SQL surface plus the `rusqlite 0.39`
//! runtime, which compiles to native targets *and* to
//! `wasm32-unknown-unknown` (via rusqlite's transparent FFI switch to
//! `sqlite-wasm-rs`).
//!
//! It deliberately does **not** depend on `cratestack-sqlx`,
//! `cratestack-axum`, or `cratestack-client-rust`. None of those compile
//! on `wasm32`, and keeping them out of the dep graph also lets backend
//! services depend on the official `sqlx` umbrella crate alongside the
//! server facade without `libsqlite3-sys` collisions.
//!
//! For backend services on Postgres, depend on
//! [`cratestack-pg`](../cratestack-pg) instead. The two crates are
//! strictly disjoint by design.
//!
//! Schema macros emit `::cratestack::*` paths, so consumers rename this
//! crate via Cargo's `package =` field:
//!
//! ```toml
//! [dependencies]
//! cratestack = { package = "cratestack-sqlite", version = "0.4" }
//! ```

pub use chrono;
pub use cratestack_core::*;
pub use cratestack_macros::{
    include_client_schema, include_embedded_schema, include_server_schema,
};
pub use cratestack_parser::{SchemaError, parse_schema, parse_schema_file, parse_schema_named};
pub use cratestack_policy::{
    PolicyExpr, PolicyLiteral, ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr,
    ProcedurePolicyLiteral, ProcedurePredicate, ReadPolicy, ReadPredicate, RelationQuantifier,
    authorize_procedure,
};

// SQL primitives shared by every backend — re-exported directly from
// `cratestack-sql` so consumers don't transit through any runtime crate.
pub use cratestack_sql::{
    CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder,
    OrderClause, Projection, RelationFilter, RelationInclude, SortDirection, SpatialFilter,
    SpatialPoint, SqlColumnValue, SqlValue, UpdateModelInput, UpsertModelInput, coalesce, point,
};

pub use regex;
pub use serde;
pub use serde_json;
pub use tracing;
pub use uuid;

// `Json<T>` is a serde-only newtype here — sqlx isn't in the dep graph,
// so the model struct can be assembled and read by the rusqlite-side
// `FromRusqliteRow` decoder without pulling tokio-net.
pub use cratestack_core::Json;

// Embedded SQLite backend — wasm32-compatible alongside native (mobile,
// desktop), via rusqlite 0.39's transparent FFI switch to `sqlite-wasm-rs`.
pub use cratestack_rusqlite as rusqlite_backend;
pub use cratestack_rusqlite::{
    DateTimeColumn, DecimalColumn, FromPartialRusqliteRow, FromRusqliteRow, JsonColumn,
    RusqliteError, RusqliteRuntime, SqlValueParam, UuidColumn, rusqlite,
};
