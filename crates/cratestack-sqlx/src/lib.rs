/// Compatibility shim that exposes a `sqlx`-shaped API by re-exporting from
/// `sqlx-core` + `sqlx-postgres` directly.
///
/// **Why this shim exists:** depending on the `sqlx` umbrella crate transitively
/// pulls `sqlx-sqlite` into the resolve graph (Cargo's resolver materialises the
/// optional dep even when no feature activates it), which pins `libsqlite3-sys
/// ^0.30.1` and conflicts with `rusqlite 0.39`'s `libsqlite3-sys ^0.37` via the
/// `links = "sqlite3"` rule. Going direct to the split crates side-steps the
/// leak entirely. Downstream users keep writing `cratestack::sqlx::X` paths;
/// macro emissions stay unchanged.
///
/// **SemVer caveat:** `sqlx-core` documents itself as "not meant for general use"
/// without SemVer guarantees. The surface re-exported here is the narrow subset
/// the umbrella `sqlx` crate exposes, which is stable in practice across `0.8.x`.
/// If `sqlx-core` breaks at a `0.8` patch, this shim adapts in one place.
pub mod sqlx {
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
}

mod audit;
mod delegate;
mod descriptor;
mod idempotency;
mod isolation;
mod migrations;
mod query;
mod render;
#[cfg(test)]
mod tests;

pub use audit::{AUDIT_TABLE_DDL, primary_key_from_snapshot, snapshot_model};
pub use idempotency::{SqlxIdempotencyStore, expiry_from};
pub use isolation::{run_in_isolated_tx, run_in_isolated_tx_with_retries};
pub use migrations::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};

pub use cratestack_policy::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate};
pub use cratestack_sql::{
    coalesce, point, CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault,
    CreateDefaultType, CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName,
    IntoSqlValue, JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey,
    NullOrder, OrderClause, RelationFilter, RelationInclude, RelationQuantifier, SortDirection,
    SpatialFilter, SpatialPoint, SqlColumnValue, SqlValue, UpdateModelInput, UpsertModelInput,
};
pub use delegate::{
    ModelDelegate, ScopedAggregate, ScopedAggregateColumn, ScopedAggregateCount, ScopedBatchCreate,
    ScopedBatchDelete, ScopedBatchGet, ScopedBatchUpdate, ScopedBatchUpsert, ScopedCreateRecord,
    ScopedDeleteMany, ScopedDeleteRecord, ScopedFindMany, ScopedFindManyWith, ScopedFindUnique,
    ScopedModelDelegate, ScopedUpdateMany, ScopedUpdateManySet, ScopedUpdateRecord,
    ScopedUpdateRecordSet, ScopedUpsertRecord,
};
pub use descriptor::SqlxRuntime;
pub use query::{
    Aggregate, AggregateColumn, AggregateCount, BatchCreate, BatchDelete, BatchGet, BatchUpdate,
    BatchUpdateItem, BatchUpsert, CreateRecord, DeleteMany, DeleteRecord, FindMany, FindManyWith,
    FindUnique, UpdateMany, UpdateManySet, UpdateRecord, UpdateRecordSet, UpsertRecord,
    create_record_with_executor, update_record_with_executor,
};
