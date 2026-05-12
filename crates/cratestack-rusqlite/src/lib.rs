//! On-device SQLite backend for the CrateStack ORM.
//!
//! Designed to run inside a mobile app via FFI: synchronous (no tokio),
//! single-connection (single-user device), and **policy-free** (authorization
//! is the app's concern on-device, not the storage layer's). The public
//! surface mirrors `cratestack-sqlx`'s ergonomics so the same `.cstack`
//! schemas drive both backends.

mod delegate;
mod render;
mod row;
mod runtime;
mod value;

pub mod ddl;
pub mod ffi;
#[cfg(target_arch = "wasm32")]
pub mod opfs;

pub use cratestack_sql::{
    CreateDefault, CreateDefaultType, CreateModelInput, FieldRef, Filter, FilterExpr,
    IntoSqlValue, ModelColumn, ModelDescriptor, OrderClause, RelationFilter, RelationQuantifier,
    SortDirection, SqlColumnValue, SqlValue, SqliteDialect, UpdateModelInput,
};

pub use delegate::{
    CreateRecord, DeleteRecord, FindMany, FindUnique, ModelDelegate, UpdateRecord, UpdateRecordSet,
};
pub use render::{render_delete, render_insert, render_select, render_select_by_pk, render_update};
pub use row::FromRusqliteRow;
pub use runtime::{RusqliteError, RusqliteRuntime};
pub use value::{
    DateTimeColumn, DecimalColumn, JsonColumn, SqlValueParam, UuidColumn, decode_datetime,
    decode_decimal, decode_json, decode_uuid,
};

pub use rusqlite;
