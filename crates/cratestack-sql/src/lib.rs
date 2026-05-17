//! Dialect-agnostic SQL primitives shared by the Postgres (`cratestack-sqlx`)
//! and SQLite (`cratestack-rusqlite`) backends.
//!
//! This crate carries the type definitions every backend agrees on:
//!
//! - [`SqlValue`] / [`SqlColumnValue`] — value envelopes used to bind data
//! - [`CreateModelInput`] / [`UpdateModelInput`] / [`UpsertModelInput`] —
//!   traits the codegen emits
//! - [`Filter`] / [`FilterExpr`] / [`FieldRef`] — query AST
//! - [`OrderClause`] / [`SortDirection`] — ordering AST
//! - [`ModelDescriptor`] / [`ModelColumn`] / [`CreateDefault`] — schema
//!   metadata baked into compiled code by `include_schema!`
//!
//! Rendering SQL strings, executing queries, and any DB-driver coupling
//! live in the backend crates.

mod descriptor;
mod dialect;
mod filter;
mod order;
mod values;

pub use descriptor::{CreateDefault, CreateDefaultType, ModelColumn, ModelDescriptor};
pub use dialect::{Dialect, PostgresDialect, SqliteDialect};
pub use filter::{
    CoalesceExpr, CoalesceFilter, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName,
    JsonFilter, JsonTextPath, RelationFilter, RelationQuantifier, SpatialFilter, SpatialPoint,
    coalesce, point,
};
pub use order::{NullOrder, OrderClause, OrderTarget, SortDirection};
pub use values::{
    ConflictTarget, CreateModelInput, FilterValue, IntoSqlValue, ModelPrimaryKey, Projection,
    RelationInclude, SqlColumnValue, SqlValue, UpdateModelInput, UpsertModelInput,
    find_duplicate_sql_value,
};
