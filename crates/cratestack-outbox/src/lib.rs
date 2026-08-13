//! The transactional outbox pattern for CrateStack applications.
//!
//! A service that emits domain events alongside a database write faces the
//! classic dual-write problem: write the row, then publish the event — and
//! a crash between the two loses the event (or publishes one for a write
//! that never committed). The outbox pattern closes that gap by writing the
//! event as an ordinary row in the *same* transaction as the business
//! write: [`OutboxClient::persist_in_tx`] takes the caller's own
//! `sqlx::Transaction` and inserts into it, so the event exists if and only
//! if the transaction that produced it committed. A separate snapshotter —
//! outside this crate — polls [`OutboxClient::drain`], paging through
//! events in `id` order and tracking its own cursor; [`axum_handler`]
//! exposes that drain (plus a retention sweep, [`OutboxClient::gc_older_than`])
//! as two HTTP endpoints a service can mount directly.
//!
//! The `id` column is a UUIDv7, minted by [`OutboxClient::persist`]/
//! [`OutboxClient::persist_in_tx`] via `Uuid::now_v7()`. UUIDv7 is
//! timestamp-prefixed and therefore lexically sortable, which is what lets
//! `drain`'s cursor stay a single opaque string (`ORDER BY id ASC WHERE id
//! > $cursor`) instead of a separate sequence column.
//!
//! ## Why this crate does not use `include_server_schema!`
//!
//! The downstream crate this was absorbed from (`events-kit`) declared a
//! full `.cstack` schema and generated a typed `cratestack::Cratestack`
//! handle via `include_server_schema!(db = Postgres)` purely to reach
//! `.pool()` on it — every actual read and write in its `OutboxClient` ran
//! raw `sqlx` against the table directly, and its own module doc recorded
//! why: cratestack's typed `Json<cratestack::Value>` column serialises a
//! `serde_json::Value` in its externally-tagged wire form
//! (`{"Map":{"key":{"String":"..."}}}`), which corrupts the plain-JSONB
//! shape a lake snapshotter (or any direct `SELECT payload`) expects. The
//! generated model's typed accessors and its `@@allow` policy checks were
//! therefore never called from anywhere in the crate.
//!
//! Using `include_server_schema!` at all would also force this crate to
//! depend on the `cratestack-pg` L5 facade — the macro is only reachable
//! through it — which would put a crate whose actual logic belongs at L2
//! behind the same layer as the schema-first facades themselves, and
//! `docs/design/layering.md` §2's L5 rule ("a facade that grows a function
//! has stopped being a facade") already rules out folding this logic into
//! `cratestack-pg` directly. A crate that only reaches L5 to throw the
//! generated model away is paying that placement cost for nothing.
//!
//! So this crate hand-writes its one table against `cratestack-sqlx`
//! directly instead — the same posture `cratestack-sqlx` itself already
//! takes for its own internal tables ([`cratestack_sqlx::AUDIT_TABLE_DDL`],
//! `MIGRATIONS_TABLE_DDL`): a bare DDL constant a caller copies into a
//! migration, plus hand-written queries against it. [`OUTBOX_EVENTS_DDL`]
//! is that constant here. The table is named `cratestack_outbox_events`
//! (not the downstream crate's bare `outbox_events`) to match that same
//! `cratestack_*`-prefixed convention and avoid colliding with an
//! application's own tables.
//!
//! ## Payload storage
//!
//! `payload` is stored as plain `JSONB` — exactly the `serde_json::Value`
//! the caller handed [`NewEvent`], with no cratestack-specific wrapping —
//! so any downstream consumer reading the table directly (a lake
//! snapshotter, `psql`) sees the value it produced, byte for byte.

pub mod axum_handler;

mod client;
mod drain;
mod envelope;
mod negotiate;

pub use axum_handler::{GcRequest, GcResponse, decode_body, drain_handler, gc_handler};
pub use client::OutboxClient;
pub use drain::{DrainRequest, DrainResponse};
pub use envelope::{EventEnvelope, NewEvent};

/// DDL for the `cratestack_outbox_events` table this crate reads and
/// writes. Each emitting service owns its own copy of this table (in its
/// own database) — copy this constant verbatim into a migration rather
/// than depending on a shared migrator, so the schema stays in lockstep
/// across services without a runtime dependency between them. Mirrors how
/// [`cratestack_sqlx::AUDIT_TABLE_DDL`] and `MIGRATIONS_TABLE_DDL` are
/// consumed.
pub const OUTBOX_EVENTS_DDL: &str = include_str!("outbox_events.sql");
