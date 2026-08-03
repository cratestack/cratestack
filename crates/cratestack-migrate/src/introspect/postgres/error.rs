//! Introspection error type. Kept separate from [`crate::MigrateError`]
//! (which is about snapshot file I/O) since this crate's default build
//! doesn't know about `sqlx` at all — `MigrateError` can't carry a
//! `sqlx_core::Error` without leaking the `postgres-introspect`
//! feature's dependency into every consumer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntrospectError {
    #[error("failed to query Postgres catalog state: {0}")]
    Query(#[from] sqlx_core::Error),
}
