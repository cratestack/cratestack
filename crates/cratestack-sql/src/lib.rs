//! Dialect-agnostic SQL primitives shared by the Postgres (`cratestack-sqlx`)
//! and SQLite (`cratestack-rusqlite`) backends.
//!
//! This crate carries the type definitions every backend agrees on:
//!
//! - [`SqlValue`] / [`SqlColumnValue`] — value envelopes used to bind data
//! - [`CreateModelInput`] / [`UpdateModelInput`] — traits the codegen emits
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
pub mod policy_render;
pub mod render;
mod values;

pub use descriptor::{CreateDefault, CreateDefaultType, ModelColumn, ModelDescriptor};
pub use dialect::{Dialect, PostgresDialect, SqliteDialect};
pub use filter::{
    FieldRef, Filter, FilterExpr, FilterOp, RelationFilter, RelationQuantifier,
};
pub use order::{OrderClause, OrderTarget, SortDirection};
pub use policy_render::{
    auth_value_to_sql, policy_literal_to_sql, render_action_policy, render_policy_expr,
    render_read_policy,
};
pub use render::{
    placeholder_string, render_filter_expr, render_filter_exprs, render_order_and_paging,
    render_order_clause, render_relation_filter, render_relation_subquery, sort_direction_sql,
    SqlSink, StringSink, NULL_ORDER_SQL,
};
pub use values::{
    CreateModelInput, FilterValue, IntoSqlValue, SqlColumnValue, SqlValue, UpdateModelInput,
};
