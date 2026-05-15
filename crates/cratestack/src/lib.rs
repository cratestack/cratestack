pub use chrono;
#[cfg(not(target_arch = "wasm32"))]
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
// `cratestack-sql` so consumers don't transit through `cratestack-sqlx`. This
// is the load-bearing change that lets `include_embedded_schema!`-generated
// code resolve `::cratestack::FilterExpr` etc. on `wasm32-unknown-unknown`,
// where sqlx can't compile.
pub use cratestack_sql::{
    coalesce, CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder, OrderClause, RelationFilter,
    SortDirection, SqlColumnValue, SqlValue, UpdateModelInput, UpsertModelInput,
};

// Embedded SQLite backend — wasm32-compatible alongside native (mobile,
// desktop), via `rusqlite 0.39`'s transparent FFI switch to `sqlite-wasm-rs`.
pub use cratestack_rusqlite as rusqlite_backend;
pub use cratestack_rusqlite::{
    DateTimeColumn, DecimalColumn, FromRusqliteRow, JsonColumn, RusqliteError, RusqliteRuntime,
    SqlValueParam, UuidColumn, rusqlite,
};

pub use regex;
pub use serde;
pub use serde_json;
pub use tracing;
pub use uuid;

// -----------------------------------------------------------------------------
// `Json<T>` resolution per target.
//
// The schema macro emits model fields as `::cratestack::Json<T>` so the same
// `cratestack_schema::Model` struct compiles on every target:
//
// - On native (server), it resolves to `sqlx::types::Json<T>` so `sqlx::FromRow`
//   decodes Postgres `jsonb` columns into it directly. Existing server code is
//   bit-identical to 0.3.0.
//
// - On `wasm32-unknown-unknown` (browser), it resolves to a serde-only newtype
//   `cratestack_core::Json<T>` — sqlx isn't in the dep graph at all on this
//   target, so the model struct can be assembled and read by the rusqlite-side
//   `FromRusqliteRow` decoder without pulling tokio-net.
// -----------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::sqlx::types::Json;

#[cfg(target_arch = "wasm32")]
pub use cratestack_core::Json;

// -----------------------------------------------------------------------------
// Server-only surface — sqlx, axum, audit/idempotency/migrations/isolation.
// Target-gated off on `wasm32-unknown-unknown` so embedded builds don't pull
// in `mio` / `tokio-net` / `sqlx-postgres`.
// -----------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_axum::axum;
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_axum::*;

// Disambiguate the `rpc` module path. Both `cratestack_core` (wire
// shapes, lifted in #31) and `cratestack_axum` (binding helpers) now
// expose an `rpc` module, so the two `pub use ..::*` globs above
// collide on the name and `::cratestack::rpc::*` resolves
// non-deterministically. Macro-emitted code in `transport rpc`
// schemas references symbols like `encode_rpc_error`,
// `convert_handler_error_response`, `response_to_frame`, and
// `RPC_BINDING_CAPABILITIES` — all of which live in `cratestack-axum::rpc`.
// An explicit `pub use` re-export takes precedence over the globs,
// pinning `::cratestack::rpc` to the axum module (which itself
// re-exports the wire types from `cratestack-core::rpc`).
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_axum::rpc;

#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::AUDIT_TABLE_DDL;
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::sqlx;
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::{
    Aggregate, AggregateColumn, AggregateCount, CreateRecord, DeleteMany, DeleteRecord, FindMany,
    FindUnique, ModelDelegate, ScopedAggregate, ScopedAggregateColumn, ScopedAggregateCount,
    ScopedCreateRecord, ScopedDeleteMany, ScopedDeleteRecord, ScopedFindMany, ScopedFindUnique,
    ScopedModelDelegate, ScopedUpdateMany, ScopedUpdateManySet, ScopedUpdateRecord,
    ScopedUpdateRecordSet, SqlxIdempotencyStore, UpdateMany, UpdateManySet, UpdateRecord,
    UpdateRecordSet, create_record_with_executor, update_record_with_executor,
};
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};
#[cfg(not(target_arch = "wasm32"))]
pub use cratestack_sqlx::{run_in_isolated_tx, run_in_isolated_tx_with_retries};

/// Crypto provider selection — banks running on FIPS-validated hardware
/// enable the `crypto-aws-lc-rs` workspace feature. The function below
/// surfaces an error early when the feature is missing so the wrong build
/// can't slip into a regulated production cluster.
///
/// Operational steps for a real FIPS deployment (out of scope for the
/// framework itself):
///
/// 1. Build the workspace with `--features crypto-aws-lc-rs`.
/// 2. Use a `aws-lc-rs`/`rustls` build configured against the vendor's
///    FIPS-validated `libcrypto`.
/// 3. Call [`install_fips_crypto_provider`] from your service's
///    `main` *before* any TLS-using code runs.
/// 4. Pin the binary's `cargo audit` report and the validated module's
///    certificate id in your release process.
pub fn install_fips_crypto_provider() -> Result<(), cratestack_core::CoolError> {
    #[cfg(feature = "crypto-aws-lc-rs")]
    {
        // The provider install is a one-line glue layer banks complete in
        // their own service binary — adding `rustls` as a direct dep here
        // would force every downstream crate to inherit the choice. Banks
        // call `rustls::crypto::aws_lc_rs::default_provider().install_default()`
        // themselves; this function exists so the feature flag has a
        // visible failure mode for builds that forget to compile it in.
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

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub mod __private {
    pub use cratestack_sqlx::SqlxRuntime;

    /// Re-exports for the macro-emitted RPC dispatcher. Not part of the
    /// public API surface — schema authors should never reference these
    /// directly. Public helpers live at `cratestack::rpc::*`.
    pub use cratestack_axum::rpc::{decode_rpc_body, encode_rpc_value, response_to_frame};
}
