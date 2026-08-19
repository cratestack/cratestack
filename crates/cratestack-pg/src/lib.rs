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
//!
//! `sqlx`/`cratestack-sqlx` sit behind the default-on `postgres` Cargo
//! feature (cratestack#329). A `db = None`-only consumer
//! (`include_server_schema!(schema, db = None)`, cratestack#328) can drop
//! `sqlx` from its dependency graph entirely:
//!
//! ```toml
//! [dependencies]
//! cratestack = { package = "cratestack-pg", version = "0.4", default-features = false }
//! ```
//!
//! See `docs/design/no-database-mode.md` for when `db = None` applies and
//! what it gives up.

// Both `cratestack_core` and `cratestack_axum` expose `codec` and
// `transport` modules, and the facade re-exports both crates with a glob.
// The overlap is intentional — consumers reach those via the originating
// crate's path, not the facade root — so silence the ambiguity warning
// rather than dropping either glob.
#![allow(ambiguous_glob_reexports)]

// Re-exported so the axum dispatch tokens `cratestack-macros` generates
// for `@stream` procedures (`crate::axum::procedure::invoke_call`) can
// reference `::cratestack::async_stream::stream!` without every
// consumer adding `async-stream` to their own `Cargo.toml`. Needed
// because the `ProcedureRegistry` trait method's `db`/`ctx` parameters
// are borrowed (`&Cratestack`/`&CratestackContext`), and — per return-position
// `impl Trait` in traits' default lifetime-capture rules — the returned
// `Stream` is only valid as long as those borrows are; wrapping the
// call in a self-contained `async_stream::stream!` generator that owns
// `db`/`ctx`/`registry`/`args` internally is what lets the resulting
// `Stream` outlive the dispatch function's own stack frame (needed
// since it travels all the way into the HTTP response body). Mirrors
// how `futures` is re-exported below for the same
// "codegen references a fixed path, consumers shouldn't have to
// duplicate the dependency" reason.
pub use async_stream;
pub use chrono;
pub use cratestack_client_rust as client_rust;
pub use cratestack_core::*;
// Re-exported (renamed from the `futures-util` crate, which is what
// actually implements it) so `@stream` procedures' generated
// `ProcedureRegistry` trait method — `impl ::cratestack::futures::Stream<
// Item = Result<T, CratestackError>> + Send` (see
// `cratestack-macros/src/procedure.rs`) — has somewhere to point without
// every consumer adding its own `futures`/`futures-core`/`futures-util`
// dependency. Mirrors how `chrono`/`uuid` are re-exported above for the
// same "codegen references a fixed path, consumers shouldn't have to
// duplicate the dependency" reason.
pub use cratestack_macros::{
    include_client_schema, include_embedded_schema, include_server_schema,
};
pub use cratestack_parser::{SchemaError, parse_schema, parse_schema_file, parse_schema_named};
pub use cratestack_policy::{
    PolicyExpr, PolicyLiteral, ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr,
    ProcedurePolicyLiteral, ProcedurePredicate, ReadPolicy, ReadPredicate, RelationQuantifier,
    authorize_procedure,
};
pub use futures_util as futures;

// SQL primitives shared by every backend — re-exported directly from
// `cratestack-sql` so consumers don't transit through `cratestack-sqlx`.
pub use cratestack_sql::{
    CoalesceExpr, CoalesceFilter, ConflictTarget, CreateDefault, CreateDefaultType,
    CreateModelInput, FieldRef, Filter, FilterExpr, FilterOp, IntoColumnName, IntoSqlValue,
    JsonFilter, JsonTextPath, ModelColumn, ModelDescriptor, ModelPrimaryKey, NullOrder,
    OrderCatalog, OrderClause, OrderRelationEdge, Orderable, Projection, ReadSource,
    RelationFilter, RelationHop, RelationInclude, ResolvedOrderTarget, SortDirection,
    SpatialFilter, SpatialPoint, SqlColumnValue, SqlValue, Unorderable, UpdateModelInput,
    UpsertModelInput, VectorDistanceExpr, VectorDistanceFilter, VectorMetric, ViewDescriptor,
    WriteSource, coalesce, is_orderable, order_value_sql, point, resolve_order_target, wrap_filter,
};

pub use regex;
pub use serde;
pub use serde_json;
pub use tracing;
pub use uuid;

// `Json<T>` resolves to `cratestack_sqlx::Json<T>` on the server so
// `sqlx::FromRow` decodes Postgres `jsonb` columns into it directly, using
// the plain/untagged codec (cratestack#162) rather than going through
// `serde_json` generically. (For `T = Value`, `Value`'s own hand-written
// `Serialize`/`Deserialize` is untagged too, since cratestack#506 — the
// two codecs agree on shape, they're just independently maintained.) This is
// only possible with the `postgres` feature enabled (cratestack#329): models
// (the only place a "Json" column is decoded from a row) can never exist
// under `db = None` (cratestack#327's guard), so a `postgres`-disabled build
// falls back to `cratestack-core`'s own backend-agnostic `Json<T>` newtype,
// which is all a `db = None` schema's procedure args/returns ever need —
// they only flow through serde (JSON/CBOR codecs), never `sqlx::FromRow`.
#[cfg(not(feature = "postgres"))]
pub use cratestack_core::json::Json;
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::Json;

// `Vector(n)` model fields decode/encode through `pgvector::Vector` at
// the sqlx boundary (see `cratestack-macros`' generated `FromRow` impl
// and `SqlValue::Vector` bind path) — re-exported so macro-emitted
// `::cratestack::pgvector::Vector` paths resolve. Requires this
// facade's own `pgvector` feature, which forwards to both
// `cratestack-macros/pgvector` (the compile-time declaration gate)
// and `cratestack-sqlx/pgvector` (the real column codec) in lockstep.
#[cfg(feature = "pgvector")]
pub use cratestack_sqlx::pgvector;

// -----------------------------------------------------------------------------
// Server surface — axum, audit/idempotency/migrations/isolation.
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

// Everything below is sqlx (Postgres)-backed and only compiled in when the
// default-on `postgres` feature is enabled (cratestack#329). A `db = None`
// -only consumer builds with `default-features = false` (or explicitly
// disables `postgres`) to drop `sqlx`/`cratestack-sqlx` from its dependency
// graph entirely — nothing generated under `db = None` ever references these
// symbols, since models (the only consumers of them) can never exist in a
// `datasource { provider = "none" }` schema.
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::AUDIT_TABLE_DDL;
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::sqlx;
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::{
    Aggregate, AggregateColumn, AggregateCount, CreateRecord, DeleteMany, DeleteRecord, FindMany,
    FindManyWith, FindUnique, FromPartialPgRow, ModelDelegate, ProjectedFindMany,
    ProjectedFindUnique, RunInTxOutcome, ScopedAggregate, ScopedAggregateColumn,
    ScopedAggregateCount, ScopedCreateRecord, ScopedDeleteMany, ScopedDeleteRecord, ScopedFindMany,
    ScopedFindManyWith, ScopedFindUnique, ScopedModelDelegate, ScopedProjectedFindMany,
    ScopedProjectedFindUnique, ScopedUpdateMany, ScopedUpdateManySet, ScopedUpdateRecord,
    ScopedUpdateRecordSet, ScopedUpsertRecord, ScopedUpsertRecordDoNothing, SqlxIdempotencyStore,
    UpdateMany, UpdateManySet, UpdateRecord, UpdateRecordSet, UpsertOutcome, UpsertRecord,
    UpsertRecordDoNothing, ViewDelegate, ViewDelegateNoUnique, create_record_with_executor,
    update_record_with_executor,
};
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::{
    MIGRATIONS_TABLE_DDL, Migration, MigrationState, MigrationStatus, apply_pending,
    ensure_migrations_table, status,
};
#[cfg(feature = "postgres")]
pub use cratestack_sqlx::{
    Tx, cratestack_error_from_sqlx, run_in_isolated_tx, run_in_isolated_tx_with_retries,
};

/// Crypto provider selection for FIPS-validated deployments.
///
/// **`crypto-aws-lc-rs` is not implemented yet.** Enabling it is a hard
/// `compile_error!`, not a working FIPS mode — this used to return `Ok(())`
/// without installing any provider, which is a false assurance in a
/// compliance-facing API: a service that called this and checked for `Ok`
/// got an affirmative return while still running on the non-FIPS `ring`
/// backend. See <https://github.com/cratestack/cratestack/issues/334>.
///
/// Making this real requires the TLS backend becoming a genuine choice
/// across `cratestack-sqlx` and `cratestack-client-rust` (both currently
/// hard-select `ring`), not just adding `aws-lc-rs` as a dependency here —
/// Cargo features are additive, so enabling `crypto-aws-lc-rs` today would
/// only add a second provider alongside `ring`, not replace it. Until that
/// backend-selection work lands, this function fails to compile under the
/// feature rather than silently lying about what it installed.
pub fn install_fips_crypto_provider() -> Result<(), cratestack_core::CratestackError> {
    #[cfg(feature = "crypto-aws-lc-rs")]
    {
        compile_error!(
            "cratestack-pg's `crypto-aws-lc-rs` feature does not install a FIPS-validated \
             crypto provider yet — see install_fips_crypto_provider's doc comment and \
             https://github.com/cratestack/cratestack/issues/334. Do not enable this feature."
        )
    }
    #[cfg(not(feature = "crypto-aws-lc-rs"))]
    {
        Err(cratestack_core::CratestackError::Internal(
            "cratestack was not compiled with `crypto-aws-lc-rs` feature; \
             FIPS-validated crypto provider is unavailable"
                .to_owned(),
        ))
    }
}

#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "postgres")]
    pub use cratestack_sqlx::SqlxRuntime;
    // Not part of the public API surface — the generated
    // `Cratestack::dispatch_audit_sink` (cratestack#534) is the
    // consumer-facing wrapper around this; see its doc comment.
    #[cfg(feature = "postgres")]
    pub use cratestack_sqlx::dispatch_audit_sink;

    /// Re-exports for the macro-emitted RPC dispatcher. Not part of the
    /// public API surface — schema authors should never reference these
    /// directly. Public helpers live at `cratestack::rpc::*`.
    pub use cratestack_axum::rpc::{decode_rpc_body, encode_rpc_value, response_to_frame};

    /// `@@subscribe` SSE dispatch (`GET /rpc/subscribe/{op_id}`, design
    /// doc §3.4a, cratestack#390): the bounded-channel bridge from a
    /// `CratestackEventBus` push callback to a `Stream`, and the encoder that
    /// turns that `Stream` into a `text/event-stream` response. Not
    /// part of the public API surface for the same reason as the rest
    /// of this module.
    pub use cratestack_axum::rpc::{
        encode_model_event_sse_response, guarded_receiver_stream, subscription_channel,
        validate_subscribe_accept_header,
    };
}
