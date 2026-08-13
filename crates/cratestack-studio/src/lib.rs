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
//! descriptor path the macro-generated server runs. Concretely, for
//! reads and writes issued against a `[target.db]` target:
//!
//! - **`@version` is never bumped.** Optimistic-concurrency `if_match`
//!   checks made by application code that read a row before a Studio
//!   edit will observe a stale-but-still-matching version and silently
//!   overwrite the edit, believing nothing changed in between.
//! - **`@@emit(...)` never writes a `cratestack_event_outbox` row.**
//!   Subscribers to model events (e.g. webhook delivery) never see a
//!   Studio-originated change.
//! - **`@@allow` policies are never evaluated**, on reads or writes.
//!   Whether that's acceptable is a per-deployment call — a direct-DB
//!   admin tool having database-level access is a coherent design, but
//!   it means a `@sensitive`/role-gated field is readable through
//!   Studio's HTTP API by anyone who can reach it, unauthenticated,
//!   regardless of the schema's declared policy.
//!
//! Because of the first two, Studio refuses `POST`/`PATCH`/`DELETE`
//! against a `[target.db]` target for any model that declares `@version`
//! or `@@emit(...)`, unless that target sets
//! [`config::TargetDb::allow_unsafe_writes`] — the refusal names the
//! specific attribute that triggered it (`403 UNSAFE_DB_WRITE`). A
//! `[target.api]` target is unaffected: those writes go through the
//! deployed service's own generated routes, which already apply
//! `@version`, `@@emit`, and `@@allow` as declared.
//!
//! See cratestack#507 for the reasoning; routing `[target.db]` writes
//! through the same descriptor path the generated server uses (rather
//! than refusing them) remains an open, unimplemented option.

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
