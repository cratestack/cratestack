//! Batch primitives — `batch_get`, `batch_create`, `batch_update`,
//! `batch_delete`, `batch_upsert`.
//!
//! Wire shape is the tRPC-style envelope from `cratestack-core`:
//! every request returns `Vec<BatchItemResult<M>>` where each item
//! carries an independent `Ok(M)` or `Err(BatchItemError)`. The outer
//! `Result` is reserved for whole-batch infrastructure failures
//! (size cap exceeded, duplicate-input rejection, DB connection lost).
//!
//! Transactional model: one outer `BEGIN`, with each mutating item
//! running in a nested `SAVEPOINT`. Per-item failures rollback to
//! their savepoint, so failed items leave no row, no audit row, no
//! event outbox entry. The non-mutating op (`batch_get`) and the
//! single-statement op (`batch_delete`) don't need savepoints — the
//! WHERE clause already filters out policy-denied / missing rows.
//!
//! Sizing: every request is capped at `BATCH_MAX_ITEMS` at the outer
//! guard. Duplicate-input keys are loud-failed at the same guard.

mod create;
mod create_item;
mod delete;
mod get;
mod update;
mod update_item;
mod upsert;
mod upsert_item;
mod upsert_sql;
mod validate;

pub use create::BatchCreate;
pub use delete::BatchDelete;
pub use get::BatchGet;
pub use update::{BatchUpdate, BatchUpdateItem};
pub use upsert::BatchUpsert;
