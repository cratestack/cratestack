//! On-device SQLite backend for the CrateStack ORM.
//!
//! Designed to run inside a mobile app via FFI: synchronous (no tokio),
//! single-connection (single-user device), and **policy-free** (authorization
//! is the app's concern on-device, not the storage layer's). The public
//! surface mirrors `cratestack-sqlx`'s ergonomics so the same `.cstack`
//! schemas drive both backends.

mod batch;
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
    coalesce, CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder,
    OrderClause, RelationFilter, RelationQuantifier, SortDirection, SqlColumnValue, SqlValue,
    SqliteDialect, UpdateModelInput, UpsertModelInput,
};

pub use batch::{
    BatchCreate, BatchDelete, BatchGet, BatchUpdate, BatchUpdateItem, BatchUpsert,
};
pub use delegate::{
    Aggregate, AggregateColumn, AggregateCount, CreateRecord, DeleteMany, DeleteRecord, FindMany,
    FindUnique, ModelDelegate, UpdateMany, UpdateManySet, UpdateRecord, UpdateRecordSet,
    UpsertRecord,
};
pub use render::{
    render_delete, render_delete_many, render_insert, render_select, render_select_by_pk,
    render_update, render_update_many, render_upsert, render_upsert_with_conflict,
};
pub use row::FromRusqliteRow;
pub use runtime::{RusqliteError, RusqliteRuntime};
pub use value::{
    DateTimeColumn, DecimalColumn, JsonColumn, SqlValueParam, UuidColumn, decode_datetime,
    decode_decimal, decode_json, decode_uuid,
};

pub use rusqlite;
