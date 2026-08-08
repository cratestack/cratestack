//! Re-exports of `IdempotencyStore` trait and DDL from cratestack-core and cratestack-sql.

pub use cratestack_core::IdempotencyStore;
pub use cratestack_sql::IDEMPOTENCY_TABLE_DDL;

/// Maximum body size the middleware will buffer when computing the hash. A
/// request beyond this returns 413 rather than risking unbounded memory.
pub(super) const MAX_BODY_BYTES: usize = cratestack_core::store::idempotency::MAX_BODY_BYTES;
