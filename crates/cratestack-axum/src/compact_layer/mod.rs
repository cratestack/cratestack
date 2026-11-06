//! The server side of compact wire keys (`docs/design/compact-wire-keys.md`
//! §5.1, proposal C): a tower layer that swaps schema-declared field names
//! for integer keys on the wire, so the handlers behind it never see them.
//!
//! # What it does
//!
//! - **Response.** When `Accept` lists a compact type
//!   (`application/vnd.cratestack.compact+cbor`, `+json`, `+cbor-seq`) and
//!   `Cratestack-Contract` names a digest the layer's [`KeyTables`] hold for
//!   the op, a `2xx` body is rewritten to integer keys, its `Content-Type`
//!   becomes the compact type and `Vary: Accept, Cratestack-Contract` is
//!   added. A `cbor-seq` stream is rewritten item by item as it flows, and
//!   the in-band error item (tag 48900) passes through unchanged. Anything
//!   else (no compact `Accept`, an unknown selector, an op the tables lack,
//!   a non-`2xx`, `/rpc/batch`) is answered exactly as without the layer.
//! - **Request.** A body sent as a compact `Content-Type` is rewritten back
//!   to names and its `Content-Type` to the plain type before dispatch. It
//!   needs a `Cratestack-Contract` selector the tables know; otherwise the
//!   layer answers `415` (`payload_type_unsupported` over RPC,
//!   `PAYLOAD_TYPE_UNSUPPORTED` over REST), the same refusal the envelope
//!   layer gives an unaccepted payload type.
//! - The compact types in `Accept` are replaced by their plain twins before
//!   dispatch, so the handler's own negotiation (CBOR or JSON, buffered or
//!   streamed) is untouched.
//!
//! # How the op is found
//!
//! The key is the one `cratestack_core::op_keys` lists: the RPC `op_id`
//! (`model.Order.list`) read from a trailing `/rpc/{op_id}`, or on REST
//! `"<METHOD> <route template>"` from axum's `MatchedPath`. Both are matched
//! by suffix, so a router nested under any prefix needs no configuration.
//!
//! # Placement
//!
//! Generated `router()` and `rpc_router()` mount it themselves, **inside**
//! their `DefaultBodyLimit`, so a layer you add around the router
//! (`IdempotencyLayer`, `RateLimitLayer`, `EnvelopeLayer`) sees the compact
//! bytes and compact payload types the client sent, and the handlers see
//! named ones. The layer buffers a compact request, bounded by the limit
//! it is built with (`413` over it, also for the named form it expands to),
//! and a buffered response, bounded by `MAX_RESPONSE_REBUFFER_BYTES`.
//!
//! To use compact keys under a COSE envelope, allow the compact types as
//! payload types: `EnvelopeLayer::builder(..).payload_media_types(
//! ["application/cbor", COMPACT_CBOR], ["application/cbor", COMPACT_CBOR])`
//! (add `COMPACT_JSON` for JSON clients). A signed client names the type in
//! `Cratestack-Payload-Type` / `Cratestack-Payload-Accept`; the envelope
//! layer hands this layer the opened payload with that `Content-Type` and
//! `Accept`, and seals the compact answer under the compact type.
//! `+cbor-seq` is never sealable, so a signed stream stays buffered CBOR.
//!
//! An `Idempotency-Key` retry must repeat the first attempt's encoding: the
//! idempotency layer hashes the bytes it sees.
//!
//! [`KeyTables`]: cratestack_core::compact::KeyTables

mod layer;
mod negotiate;
mod request;
mod resolve;
mod response;
mod seq;
mod service;
#[cfg(test)]
mod tests;

pub use layer::CompactLayer;
pub use service::CompactService;
