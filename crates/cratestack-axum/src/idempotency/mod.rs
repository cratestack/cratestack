//! Idempotency-key middleware.
//!
//! Protects mutating routes against duplicate execution. On the first request
//! with a given `Idempotency-Key`, the handler runs and the captured response
//! is persisted. Subsequent requests with the same key replay the stored
//! response if the request body hashes match, or return `422` with a
//! `idempotency_key_conflict` code if a different body is sent under the same
//! key (per the draft IETF spec).
//!
//! Usage:
//! ```ignore
//! use cratestack_axum::idempotency::{IdempotencyLayer, SqlxIdempotencyStore};
//! let store = std::sync::Arc::new(SqlxIdempotencyStore::new(pool.clone()));
//! let router = generated_router.layer(IdempotencyLayer::new(store, std::time::Duration::from_secs(24 * 3600)));
//! ```
//!
//! In Phase 1 the layer is opt-in at the consumer's router. A follow-up will
//! wire it into macro-generated routers by default, gated by a
//! `@no_idempotency` opt-out attribute already recognised by the parser.

mod hash;
mod headers;
mod layer;
mod parse;
mod record;
mod responses;
mod service;
mod store;

#[cfg(test)]
mod tests_hash;
#[cfg(test)]
mod tests_headers;
#[cfg(test)]
mod tests_parse;

pub use hash::{hash_request, is_idempotent_target_method};
pub use headers::{decode_headers, encode_headers};
pub use layer::IdempotencyLayer;
pub use parse::parse_idempotency_key;
pub use record::{IdempotencyRecord, ReservationOutcome};
pub use service::IdempotencyService;
pub use store::{IDEMPOTENCY_TABLE_DDL, IdempotencyStore};
