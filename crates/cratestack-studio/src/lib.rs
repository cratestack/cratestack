//! CrateStack Studio — admin and testing surface for `.cstack` schemas.
//!
//! Phase 0 surface: `init` writes a starter `studio.toml`, `run` boots an
//! Axum server that serves a stub page. The full data layer, UI, and API
//! land in subsequent phases (see workspace plan).
//!
//! ## `[target.db]` is not the generated API
//!
//! A `[target.db]` target ([`config::TargetDb`]) is a direct SQL
//! connection Studio opens itself — it does **not** route through the
//! descriptor path the macro-generated server runs. cratestack#507
//! reported that writes through it used to skip `@version` bumping and
//! `@@emit` outbox rows silently; #553 closed most of that gap by
//! routing writes through the same primitives the generated server uses
//! wherever a backend can actually honor them. What's true today, per
//! attribute:
//!
//! - **`@version` is always bumped for real, on every backend.** A
//!   `POST`/`PATCH`/`DELETE` against a `@version` model through a
//!   `[target.db]` target increments the column server-side — Postgres
//!   and SQLite alike — exactly like the generated server does. It is
//!   never refused and never needs `allow_unsafe_writes`; there is
//!   nothing left to bypass here.
//! - **`@@emit(...)` writes a real `cratestack_event_outbox` row, but
//!   only on a Postgres `[target.db]` target**, in the same transaction
//!   as the row mutation, via the same
//!   `cratestack_sqlx::enqueue_event_outbox` primitive the generated
//!   server's descriptor path uses. **On a SQLite `[target.db]` target
//!   this is a permanent backend capability difference, not a gap
//!   waiting to be closed**: `cratestack-rusqlite` has no event-outbox
//!   table at all, and `include_embedded_schema!` itself treats
//!   `@@emit(...)` as a no-op — the framework's own embedded backend has
//!   never implemented it (see that macro's doc comment and the
//!   `## 0.4.0` CHANGELOG entry). Studio would be inventing a
//!   Studio-only guarantee the framework doesn't provide anywhere else
//!   if it built SQLite an outbox just for this admin surface, so it
//!   doesn't: a `POST`/`PATCH`/`DELETE` against an `@@emit(...)` model on
//!   a SQLite `[target.db]` target still returns `403 UNSAFE_DB_WRITE`
//!   unless that target sets [`config::TargetDb::allow_unsafe_writes`].
//! - **`@@allow` policies are never evaluated**, on reads or writes, on
//!   either driver. Whether that's acceptable is a per-deployment call —
//!   a direct-DB admin tool having database-level access is a coherent
//!   design, but it means a `@sensitive`/role-gated field is readable
//!   through Studio's HTTP API by anyone who can reach it,
//!   unauthenticated, regardless of the schema's declared policy. This
//!   is unchanged by #553 and remains deliberately out of scope.
//!
//! The refusal (`403 UNSAFE_DB_WRITE`, naming the specific attribute
//! that triggered it) only ever fires for an unroutable `@@emit(...)` on
//! a non-Postgres `[target.db]` target now — never for `@version` alone,
//! and never on Postgres. A `[target.api]` target is unaffected either
//! way: those writes go through the deployed service's own generated
//! routes, which already apply `@version`, `@@emit`, and `@@allow` as
//! declared. See cratestack#507 and #553 for the full history.

pub mod api;
pub mod audit;
pub mod config;
pub mod data;
pub mod eject;
pub mod search;
pub mod server;
pub mod snippet;
pub mod validators;
pub mod workspace;

#[cfg(feature = "embed-ui")]
pub mod ui_assets;

pub use eject::{EjectError, EjectOptions, EjectReport, eject};
pub use workspace::{LoadedTarget, LoadedWorkspace, WorkspaceError};

pub use config::{StudioConfig, StudioConfigError, TargetConfig, TargetMode};
pub use server::{ServerError, ServerOptions, run};

/// Default address the studio binds when no override is provided.
pub const DEFAULT_BIND: &str = "127.0.0.1:7878";

/// Default config file name resolved relative to the current directory.
pub const DEFAULT_CONFIG_FILE: &str = "studio.toml";

/// Default starter `studio.toml` body written by `studio init`.
pub const STARTER_CONFIG: &str = include_str!("../starter/studio.toml");
