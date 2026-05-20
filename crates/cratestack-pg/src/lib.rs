//! CrateStack server facade — Postgres (sqlx) + Axum.
//!
//! This crate is the server-side slice of the framework. It re-exports the
//! shared schema / parser / policy / SQL surface plus the sqlx (Postgres)
//! runtime, Axum HTTP bindings, and the generated Rust client runtime.
//!
//! It deliberately does **not** depend on `cratestack-rusqlite`. That keeps
//! `libsqlite3-sys` out of the dep graph, so consumers can use the official
//! `sqlx` umbrella crate (which optionally declares `sqlx-sqlite` and trips
//! Cargo's `links = "sqlite3"` collision rule) without needing a local
//! `sqlx-shim` workaround.
//!
//! For embedded / mobile / wasm targets, depend on `cratestack-sqlite`
//! instead. The two crates are strictly disjoint by design.
//!
//! Schema macros emit `::cratestack::*` paths, so consumers rename this
//! crate via Cargo's `package =` field:
//!
//! ```toml
//! [dependencies]
//! cratestack = { package = "cratestack-pg", version = "0.4" }
//! ```

pub use chrono;
pub use cratestack_client_rust as client_rust;
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
// `cratestack-sql` so consumers don't transit through `cratestack-sqlx`.
pub use cratestack_sql::{
    CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder,
    OrderClause, Projection, ReadSource, RelationFilter, RelationInclude, SortDirection,
    SpatialFilter, SpatialPoint, SqlColumnValue, SqlValue, UpdateModelInput, UpsertModelInput,
    ViewDescriptor, WriteSource, coalesce, point,
};

pub use regex;
pub use serde;
pub use serde_json;
pub use tracing;
pub use uuid;

// `Json<T>` resolves to `sqlx::types::Json<T>` on the server so
// `sqlx::FromRow` decodes Postgres `jsonb` columns into it directly.
pub use cratestack_sqlx::sqlx::types::Json;

// -----------------------------------------------------------------------------
// Server surface — sqlx, axum, audit/idempotency/migrations/isolation.
// -----------------------------------------------------------------------------

pub use cratestack_axum::axum;
pub use cratestack_axum::*;

// Disambiguate the `rpc` module path. Both `cratestack_core` (wire shapes)
// and `cratestack_axum` (binding helpers) expose an `rpc` module, so the
// two `pub use ..::*` globs collide on the name and `::cratestack::rpc::*`
// resolves non-deterministically. Macro-emitted code in `transport rpc`
// schemas references symbols like `encode_rpc_error`,
// `convert_handler_error_response`, `response_to_frame`, and
// `RPC_BINDING_CAPABILITIES` — all of which live in `cratestack-axum::rpc`.
// An explicit `pub use` re-export takes precedence over the globs, pinning
// `::cratestack::rpc` to the axum module (which itself re-exports the wire
// types from `cratestack-core::rpc`).
pub use cratestack_axum::rpc;

pub use cratestack_sqlx::AUDIT_TABLE_DDL;
pub use cratestack_sqlx::sqlx;
pub use cratestack_sqlx::{
    Aggregate, AggregateColumn, AggregateCount, CreateRecord, DeleteMany, DeleteRecord, FindMany,
    FindManyWith, FindUnique, FromPartialPgRow, ModelDelegate, ProjectedFindMany,
    ProjectedFindUnique, ScopedAggregate, ScopedAggregateColumn, ScopedAggregateCount,
    ScopedCreateRecord, ScopedDeleteMany, ScopedDeleteRecord, ScopedFindMany, ScopedFindManyWith,
    ScopedFindUnique, ScopedModelDelegate, ScopedProjectedFindMany, ScopedProjectedFindUnique,
    ScopedUpdateMany, ScopedUpdateManySet, ScopedUpdateRecord, ScopedUpdateRecordSet,
    SqlxIdempotencyStore, UpdateMany, UpdateManySet, UpdateRecord, UpdateRecordSet, ViewDelegate,
    ViewDelegateNoUnique, create_record_with_executor, update_record_with_executor,
};
pub use cratestack_sqlx::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};
pub use cratestack_sqlx::{cool_error_from_sqlx, run_in_isolated_tx, run_in_isolated_tx_with_retries};

/// Crypto provider selection — banks running on FIPS-validated hardware
/// enable the `crypto-aws-lc-rs` feature. The function below surfaces an
/// error early when the feature is missing so the wrong build can't slip
/// into a regulated production cluster.
///
/// Operational steps for a real FIPS deployment (out of scope for the
/// framework itself):
///
/// 1. Build with `--features crypto-aws-lc-rs`.
/// 2. Use an `aws-lc-rs`/`rustls` build configured against the vendor's
///    FIPS-validated `libcrypto`.
/// 3. Call [`install_fips_crypto_provider`] from your service's `main`
///    *before* any TLS-using code runs.
/// 4. Pin the binary's `cargo audit` report and the validated module's
///    certificate id in your release process.
pub fn install_fips_crypto_provider() -> Result<(), cratestack_core::CoolError> {
    #[cfg(feature = "crypto-aws-lc-rs")]
    {
        Ok(())
    }
    #[cfg(not(feature = "crypto-aws-lc-rs"))]
    {
        Err(cratestack_core::CoolError::Internal(
            "cratestack was not compiled with `crypto-aws-lc-rs` feature; \
             FIPS-validated crypto provider is unavailable"
                .to_owned(),
        ))
    }
}

#[doc(hidden)]
pub mod __private {
    pub use cratestack_sqlx::SqlxRuntime;

    /// Re-exports for the macro-emitted RPC dispatcher. Not part of the
    /// public API surface — schema authors should never reference these
    /// directly. Public helpers live at `cratestack::rpc::*`.
    pub use cratestack_axum::rpc::{decode_rpc_body, encode_rpc_value, response_to_frame};
}
