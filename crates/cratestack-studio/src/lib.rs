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
//! - **No schema-declared write constraint is enforced at all** — not
//!   `@@allow`/`@@deny` (on reads or writes, on either driver), and not
//!   `@@internal(...)` route suppression. A model whose `create` is
//!   suppressed by `@@internal("create")`, or denied outright by an
//!   `@@allow` rule, is still creatable through `POST
//!   /api/targets/{key}/models/{model}/records` on an `rw`
//!   `[target.db]` target. Studio's records API evaluates no policy and
//!   consults no suppression metadata: `api/records/guards.rs` gates on
//!   [`TargetMode::Rw`] and on write *routability*, nothing else.
//!   Whether that's acceptable is a per-deployment call — a direct-DB
//!   admin tool having database-level access is a coherent design, and
//!   cratestack#744 decided deliberately to keep it (option 3:
//!   document, don't enforce) — but it means a `@sensitive`/role-gated
//!   field is readable, and a deliberately-suppressed action
//!   performable, through Studio's HTTP API by anyone who can reach it,
//!   unauthenticated, regardless of what the schema declares. Every
//!   successful write is recorded in the audit log ([`audit`]) — that
//!   is the compensating control here, and it is detective, not
//!   preventive.
//!
//! The refusal (`403 UNSAFE_DB_WRITE`, naming the specific attribute
//! that triggered it) only ever fires for an unroutable `@@emit(...)` on
//! a non-Postgres `[target.db]` target now — never for `@version` alone,
//! and never on Postgres. See cratestack#507 and #553 for the full
//! history.
//!
//! ## Granting `rw`: which of the two channels you are granting
//!
//! `mode = "rw"` grants very different things depending on the channel
//! a target resolves to, and that is a config-time choice rather than a
//! per-request one: [`workspace`]'s loader takes `[target.db]` whenever
//! the target declares one, and falls back to `[target.api]` only when
//! there is no `[target.db]` block at all. A target declaring **both**
//! is a `[target.db]` target for every read and write — adding
//! `[target.api]` alongside a `[target.db]` buys no enforcement.
//! (`[target.api].prefer_for` is parsed but not consulted by anything
//! today; it does not redirect writes.)
//!
//! - **`rw` on a `[target.db]` target is database-level access**, and
//!   bypasses every schema-declared write constraint as described
//!   above. Read it as "this operator may do anything `psql` could do".
//! - **`rw` on a `[target.api]`-only target grants no more than the
//!   configured credential already has.** [`data::api::ApiSource`]
//!   issues ordinary HTTP requests against the deployed service's
//!   macro-generated REST routes — the same surface the TypeScript and
//!   Dart clients consume — so `@@allow`/`@@deny` are evaluated
//!   server-side against the identity in `[target.api].auth`, exactly
//!   as for any other client, and an `@@internal(...)`-suppressed verb
//!   has no route to call: the request fails at the HTTP layer with the
//!   same `405`/`404` any other caller gets (cratestack#743, and
//!   `docs/design/route-suppression.md` §4 for which of the two).
//!   Studio implements none of this itself — the constraint is enforced
//!   by the service, so it cannot drift out of sync with the schema the
//!   service was generated from.

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
