/// Compatibility shim that exposes a `sqlx`-shaped API by re-exporting from
/// `sqlx-core` + `sqlx-postgres` directly.
///
/// **Why this shim exists:** depending on the `sqlx` umbrella crate transitively
/// pulls `sqlx-sqlite` into the resolve graph (Cargo's resolver materialises the
/// optional dep even when no feature activates it), which pins `libsqlite3-sys
/// ^0.30.1` and conflicts with `rusqlite 0.40`'s `libsqlite3-sys ^0.38` via the
/// `links = "sqlite3"` rule. Going direct to the split crates side-steps the
/// leak entirely. Downstream users keep writing `cratestack::sqlx::X` paths;
/// macro emissions stay unchanged.
///
/// **SemVer caveat:** `sqlx-core` documents itself as "not meant for general use"
/// without SemVer guarantees. The surface re-exported here is the narrow subset
/// the umbrella `sqlx` crate exposes, which was stable in practice across `0.8.x`
/// and is now pinned at `=0.9.0`. That design paid off at the 0.8→0.9 boundary:
/// upstream's `SqlSafeStr` bound and the `QueryBuilder` lifetime removal landed
/// as *additions to this list* (`AssertSqlSafe`/`SqlSafeStr`/`SqlStr`) plus
/// mechanical call-site edits, with no downstream `::cratestack::sqlx::…` path
/// changing. Treat any future minor the same way: adapt here first.
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
}

mod audit;
mod delegate;
mod descriptor;
mod error;
mod idempotency;
mod isolation;
mod json;
mod migrations;
mod partial_row;
mod query;
mod render;
#[cfg(feature = "postgis")]
mod spatial;
#[cfg(test)]
mod tests_coalesce;
#[cfg(test)]
mod tests_create_defaults;
#[cfg(test)]
mod tests_descriptor;
#[cfg(test)]
mod tests_field_filter;
#[cfg(test)]
mod tests_filter_logic;
#[cfg(test)]
mod tests_geography;
#[cfg(test)]
mod tests_json;
#[cfg(test)]
mod tests_nested_relation_policy;
#[cfg(test)]
mod tests_optional;
#[cfg(test)]
mod tests_pgvector;
#[cfg(test)]
mod tests_policy_precedence_bug;
#[cfg(test)]
mod tests_read_policy_field_predicates;
#[cfg(test)]
mod tests_read_policy_predicates;
#[cfg(test)]
mod tests_relation;
#[cfg(test)]
mod tests_system_principal_policy;
#[cfg(test)]
mod tests_update;
#[cfg(test)]
mod tests_update_many;
#[cfg(test)]
mod tests_upsert_conflict_predicate;
mod transaction;

pub use partial_row::FromPartialPgRow;

pub use json::Json;
/// Re-exported so generated code (and the facade crates) can reach
/// `::cratestack::pgvector::Vector` without depending on the
/// `pgvector` crate directly — mirrors how `sqlx` above is re-exposed
/// as a shim rather than depended on separately by every consumer.
#[cfg(feature = "pgvector")]
pub use pgvector;

/// Row-decode adapter for PostGIS `geography`/`geometry` columns
/// (cratestack#842) — re-exported so generated code can name
/// `::cratestack::Ewkb` without depending on this crate's internals.
#[cfg(feature = "postgis")]
pub use spatial::Ewkb;

pub use audit::{
    AUDIT_TABLE_DDL, RunInTxOutcome, dispatch_audit_sink, primary_key_from_snapshot, snapshot_model,
};
pub use error::cratestack_error_from_sqlx;
pub use idempotency::{SqlxIdempotencyStore, expiry_from};
pub use isolation::{run_in_isolated_tx, run_in_isolated_tx_with_retries};
pub use migrations::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};
pub use transaction::Tx;

pub use cratestack_policy::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate};
pub use cratestack_sql::{
    CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder,
    OrderClause, Orderable, Projection, RelationFilter, RelationHop, RelationInclude,
    RelationQuantifier, SortDirection, SpatialFilter, SpatialPoint, SqlColumnValue, SqlValue,
    Unorderable, UpdateModelInput, UpsertModelInput, VectorDistanceExpr, VectorDistanceFilter,
    VectorMetric, coalesce, is_orderable, order_value_sql, point, wrap_filter,
};
pub use delegate::{
    ModelDelegate, ScopedAggregate, ScopedAggregateColumn, ScopedAggregateCount, ScopedBatchCreate,
    ScopedBatchDelete, ScopedBatchGet, ScopedBatchUpdate, ScopedBatchUpsert, ScopedCreateRecord,
    ScopedDeleteMany, ScopedDeleteRecord, ScopedFindMany, ScopedFindManyWith, ScopedFindUnique,
    ScopedModelDelegate, ScopedProjectedFindMany, ScopedProjectedFindUnique, ScopedUpdateMany,
    ScopedUpdateManySet, ScopedUpdateRecord, ScopedUpdateRecordSet, ScopedUpsertRecord,
    ScopedUpsertRecordDoNothing, ViewDelegate, ViewDelegateNoUnique,
};
pub use descriptor::{SqlxRuntime, enqueue_event_outbox, ensure_event_outbox_table};
pub use query::{
    Aggregate, AggregateColumn, AggregateCount, BatchCreate, BatchDelete, BatchGet, BatchUpdate,
    BatchUpdateItem, BatchUpsert, CreateRecord, DeleteMany, DeleteRecord, FindMany, FindManyWith,
    FindUnique, ProjectedFindMany, ProjectedFindUnique, UpdateMany, UpdateManySet, UpdateRecord,
    UpdateRecordSet, UpsertOutcome, UpsertRecord, UpsertRecordDoNothing,
    create_record_with_executor, update_record_with_executor,
};
