pub mod sqlx;

mod audit;
mod bound;
mod delegate;
mod descriptor;
mod error;
mod idempotency;
mod isolated_run;
mod isolation;
mod json;
mod migrations;
mod partial_row;
mod query;
mod render;
mod retriable;
#[cfg(feature = "postgis")]
mod spatial;
#[cfg(test)]
mod tests_bigint;
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
mod tests_relation_scope;
#[cfg(test)]
mod tests_relation_scope_fixtures;
#[cfg(test)]
mod tests_relation_scope_parity;
#[cfg(test)]
mod tests_relation_scope_self;
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
    OrderClause, Orderable, Projection, RelatedReadScope, RelationFilter, RelationHop,
    RelationInclude, RelationQuantifier, SortDirection, SqlColumnValue, SqlValue, Unorderable,
    UpdateModelInput, UpsertModelInput, VectorDistanceExpr, VectorDistanceFilter, VectorMetric,
    coalesce, is_orderable, order_value_sql, wrap_filter,
};
/// PostGIS query surface (cratestack#842), gated in `cratestack-sql`
/// and forwarded through this crate's own `postgis` feature.
#[cfg(feature = "postgis")]
pub use cratestack_sql::{SpatialDistanceExpr, SpatialFilter, SpatialPoint, point};
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
