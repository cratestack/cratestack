//! CrateStack server facade for procedures-only, no-database services.
//!
//! This crate is the `db = None` slice of the framework (epic #326). It
//! re-exports the shared schema / parser / policy / SQL surface plus the
//! Axum HTTP bindings and the generated Rust client runtime — everything a
//! `datasource { provider = "none" }` server needs for routing, procedure
//! dispatch, and REST/RPC transport, minus a database backend.
//!
//! It deliberately does **not** depend on `cratestack-sqlx` — not behind a
//! feature flag, genuinely absent from `Cargo.toml`. `datasource { provider
//! = "none" }` schemas can never declare a `model` (enforced at parse time,
//! cratestack#327), and `db = Postgres` codegen is the only path that ever
//! references sqlx-backed symbols (`::cratestack::sqlx::PgPool`, the
//! `Json<T>` sqlx variant, `SqlxRuntime`, …) — so a facade that structurally
//! never has those symbols to offer can only ever support `db = None`. A
//! schema compiled with `include_server_schema!(schema, db = Postgres)`
//! under this crate fails to compile with a single, clear `compile_error!`
//! (cratestack#347's `guard_server_postgres_backend`, in
//! `cratestack-macros/src/include/datasource_guard.rs`) rather than a wall
//! of unrelated "cannot find `sqlx`/`SqlxRuntime` in `cratestack`" errors —
//! see this crate's `README.md` for the exact reproduction and transcript.
//!
//! `transport rpc` and REST (the default) both work fully under
//! `db = None` — see `docs/design/rpc-transport.md` and
//! `docs/design/no-database-mode.md`.
//!
//! `cratestack-pg` (with `default-features = false` to drop its `postgres`
//! feature) also supports `db = None` and continues to work — this crate
//! doesn't replace that path, it just names the "I never touch Postgres"
//! case directly instead of asking a consumer to depend on a crate named
//! for the database backend they're explicitly opting out of.
//!
//! Schema macros emit `::cratestack::*` paths, so consumers rename this
//! crate via Cargo's `package =` field:
//!
//! ```toml
//! [dependencies]
//! cratestack = { package = "cratestack-api", version = "0.6" }
//! ```
//!
//! ```ignore
//! cratestack::include_server_schema!("schema/foo.cstack", db = None);
//! ```
//!
//! See `docs/design/no-database-mode.md` for the full `db = None` design
//! and this crate's `README.md` for a quick-start.

// Both `cratestack_core` and `cratestack_axum` expose `codec` and
// `transport` modules, and this facade re-exports both crates with a glob.
// The overlap is intentional — consumers reach those via the originating
// crate's path, not the facade root — so silence the ambiguity warning
// rather than dropping either glob. Mirrors `cratestack-pg`.
#![allow(ambiguous_glob_reexports)]

// Re-exported so the axum dispatch tokens `cratestack-macros` generates for
// `@stream` procedures (`crate::axum::procedure::invoke_call`) can reference
// `::cratestack::async_stream::stream!` without every consumer adding
// `async-stream` to their own `Cargo.toml`. See `cratestack-pg`'s doc
// comment for the full lifetime-capture rationale — identical here, since
// `@stream` procedures are shared codegen, not `db`-conditional.
pub use async_stream;
pub use chrono;
pub use cratestack_client_rust as client_rust;
pub use cratestack_core::*;
// Re-exported (renamed from the `futures-util` crate, which is what
// actually implements it) so `@stream` procedures' generated
// `ProcedureRegistry` trait method has somewhere to point without every
// consumer adding its own `futures`/`futures-core`/`futures-util`
// dependency. Mirrors `cratestack-pg`.
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
// `cratestack-sql` so consumers don't transit through a runtime crate.
// `db = None` schemas never construct these (no models), but procedure
// codegen references some of the same shared descriptor types, so this
// mirrors `cratestack-pg`'s re-export list rather than trimming it.
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

// `Json<T>` is a serde-only newtype here — there is no `postgres` feature
// to switch on, and there never will be: `cratestack-sqlx` is not a
// dependency of this crate under any feature. `db = None` schemas only ever
// need a codec-friendly `Json<T>` for procedure args/returns (never
// `sqlx::FromRow` row decoding, since models can't exist), so this is the
// only `Json` this crate ever needs to offer. Compare `cratestack-pg`,
// which switches between this same type and `cratestack_sqlx::sqlx::types::
// Json` behind its `postgres` feature.
pub use cratestack_core::json::Json;

// -----------------------------------------------------------------------------
// Server surface — axum, audit/idempotency/isolation. No migrations module:
// that's sqlx (Postgres schema migrations)-only and has no `db = None`
// equivalent (there is no schema to migrate without a database).
// -----------------------------------------------------------------------------

pub use cratestack_axum::axum;
pub use cratestack_axum::*;

// Disambiguate the `rpc` module path. Both `cratestack_core` (wire shapes)
// and `cratestack_axum` (binding helpers) expose an `rpc` module, so the two
// `pub use ..::*` globs collide on the name and `::cratestack::rpc::*`
// resolves non-deterministically. Macro-emitted code in `transport rpc`
// schemas references symbols like `encode_rpc_error`,
// `convert_handler_error_response`, `response_to_frame`, and
// `RPC_BINDING_CAPABILITIES` — all of which live in `cratestack-axum::rpc`.
// An explicit `pub use` re-export takes precedence over the globs, pinning
// `::cratestack::rpc` to the axum module. Mirrors `cratestack-pg`.
pub use cratestack_axum::rpc;

#[doc(hidden)]
pub mod __private {
    /// Re-exports for the macro-emitted RPC batch dispatcher
    /// (`crates/cratestack-macros/src/include/server/rpc_module/batch.rs`).
    /// Not part of the public API surface — schema authors should never
    /// reference these directly. Public helpers live at
    /// `cratestack::rpc::*`.
    ///
    /// `SqlxRuntime` is deliberately **not** re-exported here — it's the
    /// `db = Postgres` runtime handle, which this crate cannot offer
    /// without `cratestack-sqlx`.
    pub use cratestack_axum::rpc::{decode_rpc_body, encode_rpc_value, response_to_frame};
}
