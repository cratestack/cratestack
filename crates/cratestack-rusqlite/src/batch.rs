//! Batch primitives — embedded mirror of `cratestack-sqlx`'s batch module.
//!
//! The embedded layer trims the surface: no policies, no audit, no event
//! outbox. That makes the implementation noticeably simpler than the
//! server side — two single-statement ops (`batch_get` / `batch_delete`)
//! and three savepointed loops (`batch_create` / `batch_update` /
//! `batch_upsert`).
//!
//! Error vocabulary: per-item failures surface as
//! `BatchItemError { code: "DATABASE_ERROR", ... }` (or `"CONFLICT"` for
//! unique-constraint violations on create / upsert), matching the codes
//! the server side projects from `CoolError`. This keeps cross-platform
//! clients on a single error-code table whether the response came from
//! sqlx or rusqlite.

mod create;
mod delete;
mod get;
mod support;
mod update;
mod upsert;

pub use create::BatchCreate;
pub use delete::BatchDelete;
pub use get::BatchGet;
pub use update::{BatchUpdate, BatchUpdateItem};
pub use upsert::BatchUpsert;
