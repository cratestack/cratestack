//! Experimental offline sync engine for CrateStack (ADR 0020, *Proposed*).
//!
//! The design is written up in `docs/design/sync-engine.md`; this crate is
//! the prototype that design was checked against. Three pieces:
//!
//! - [`protocol`] — the wire messages both halves agree on. Always compiled.
//! - `server` (feature `server`) — a commit-ordered Postgres change log fed
//!   by triggers, server-authoritative mutation replay, TTL garbage
//!   collection, and an axum router exposing `register` / `push` / `pull` /
//!   `poke`.
//! - `client` (feature `client`) — a sans-IO engine over `rusqlite`: a
//!   pending-mutation log, an undo log (shadow tables) for rebasing local
//!   writes onto server state, and the cursor. It does no networking, so it
//!   compiles to `wasm32-unknown-unknown` and drops into the existing
//!   embedded/OPFS setup unchanged.
//!
//! The model is *server-authoritative mutation replay*: clients send named
//! mutations, the server re-executes them through the normal policy-checked
//! write path, and the change log is the only way state flows back down.
//! Clients are never trusted — the embedded runtime enforces no policy, so
//! everything a client claims is re-checked on the server.

pub mod policy;
pub mod protocol;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

pub use policy::{PolicyError, SyncPolicy};
