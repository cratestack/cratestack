//! The JS <-> Rust value bridge for the `encode`/`decode` boundary.
//!
//! [`JsCborValue`] wraps [`cratestack_core::Value`] — the framework's own
//! canonical wire value — and gives it the napi `FromNapiValue` /
//! `ToNapiValue` conversions this addon's entry points need. All CBOR
//! encode/decode logic still lives in `CborCodec`; nothing here touches
//! the wire format.
//!
//! ## Why not `serde_json::Value` (cratestack#783)
//!
//! This boundary used to hand napi a `serde_json::Value`. That type has
//! no byte-string variant, and napi's `FromNapiValue for
//! serde_json::Value` classifies anything non-array and object-typed —
//! including a `Uint8Array` — as a plain object, so `new
//! Uint8Array([1,2,3,4])` reached the codec as `{"0":1,"1":2,"2":3,"3":4}`
//! and went out as a CBOR **map**. A generated Rust `Bytes` field
//! (`Vec<u8>`) cannot decode that at all, so the request failed at the
//! codec with `400 invalid request payload` and never reached the
//! handler; callers had to write `Array.from(bytes)` by hand at every
//! call site, at roughly twice the wire cost of the byte string they
//! wanted.
//!
//! `Value` has a `Bytes` variant whose `Serialize` branches on
//! `is_human_readable()` — `minicbor-serde` reports `false`, so it takes
//! the `serialize_bytes` path and lands on the wire as RFC 8949 major
//! type 2 — and whose `Deserialize` accepts the same shape back through
//! `visit_bytes`/`visit_byte_buf`. That makes the JS-side mapping in this
//! module the only missing piece:
//!
//! | JS                                          | CBOR                          |
//! |---------------------------------------------|-------------------------------|
//! | `Uint8Array` (incl. Node `Buffer`), `ArrayBuffer` | byte string (major type 2) |
//! | `number[]`                                  | array of integers (unchanged) |
//!
//! That set is exactly what `serde-wasm-bindgen` recognises on the
//! `@cratestack/cbor-web` side, and it is matched here on purpose: a
//! TypeScript client must put the same payload on the wire whichever
//! build it loads, so the node build deliberately does *not* accept more
//! than the web build can. Everything else — `Uint8ClampedArray`,
//! `DataView`, `Int32Array`, `Float64Array`, … — keeps its previous
//! object-of-indices behaviour rather than being silently reinterpreted.
//! (What the two builds do with an *unsupported* shape has always
//! differed and still does: this one degrades it to an object of indices,
//! the web one rejects it. Neither is a shape to rely on.) Callers
//! holding one of those should pass
//! `new Uint8Array(view.buffer, view.byteOffset, view.byteLength)`.
//!
//! ## Two deliberate consequences of the switch
//!
//! Both are edge cases of `Value`'s number model, which is already the
//! framework's wire contract everywhere else (`Json` fields, procedure
//! `Json` arguments, RPC error details):
//!
//! - `Value` has no unsigned-64 variant, so an integer above `i64::MAX`
//!   (a JS `BigInt` past 9223372036854775807) degrades to a float instead
//!   of staying an exact CBOR unsigned integer. Values up to `i64::MAX`
//!   are exact, and those above `2^53 - 1` still surface in JS as a
//!   `BigInt` — [`JsCborValue`]'s `Int` arm routes through
//!   `serde_json::Number` precisely to keep napi's existing
//!   safe-integer/`BigInt` split.
//! - A non-finite float (`NaN`, `±Infinity`) now survives as itself.
//!   `serde_json::Value` cannot hold one, so it used to decode as `null`.

use cratestack_core::Value;

/// Borrowing/owning wrapper around [`cratestack_core::Value`] carrying
/// this crate's JS conversions. See the module docs.
pub struct JsCborValue(pub Value);

#[cfg(not(target_arch = "wasm32"))]
mod napi_conversions;

#[cfg(test)]
mod tests;
