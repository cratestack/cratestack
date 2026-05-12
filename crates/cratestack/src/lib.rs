pub use chrono;
pub use cratestack_axum::axum;
pub use cratestack_axum::*;
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
pub use cratestack_sqlx::AUDIT_TABLE_DDL;
pub use cratestack_sqlx::sqlx;
pub use cratestack_sqlx::{
    CreateDefault, CreateDefaultType, CreateModelInput, CreateRecord, DeleteRecord, FieldRef,
    Filter, FilterExpr, FindMany, FindUnique, IntoSqlValue, ModelColumn, ModelDelegate,
    ModelDescriptor, OrderClause, RelationFilter, ScopedCreateRecord, ScopedDeleteRecord,
    ScopedFindMany, ScopedFindUnique, ScopedModelDelegate, ScopedUpdateRecord,
    ScopedUpdateRecordSet, SortDirection, SqlColumnValue, SqlValue, SqlxIdempotencyStore,
    UpdateModelInput, UpdateRecord, UpdateRecordSet, create_record_with_executor,
    update_record_with_executor,
};
pub use cratestack_sqlx::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};
pub use cratestack_sqlx::{run_in_isolated_tx, run_in_isolated_tx_with_retries};

// On-device SQLite backend. Always compiled in for now — Phase 4's choice
// (keep both backends always available) trades binary size on the server
// for a single uniform API across server and device. A future feature flag
// can hide rusqlite for size-sensitive builds.
pub use cratestack_rusqlite::{
    DateTimeColumn, DecimalColumn, FromRusqliteRow, JsonColumn, RusqliteError, RusqliteRuntime,
    SqlValueParam, UuidColumn, rusqlite,
};
pub use cratestack_rusqlite as rusqlite_backend;

pub use regex;
pub use serde;
pub use serde_json;
pub use tracing;
pub use uuid;

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

#[doc(hidden)]
pub mod __private {
    pub use cratestack_sqlx::SqlxRuntime;
}
