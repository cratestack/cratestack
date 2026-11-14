//! Native CBOR<->JSON bridge over `cratestack-codec-cbor`'s `CborCodec`,
//! for `flutter_rust_bridge` (cratestack#563 — `cratestack_cbor` on
//! pub.dev). Mirrors the shape of the two existing platform bindings —
//! `cratestack-cbor-napi` (`@cratestack/cbor-node`) and
//! `cratestack-cbor-wasm` (`@cratestack/cbor-web`) — but the codec logic
//! is absorbed into this crate rather than a fourth standalone glue
//! crate: `cratestack-client-flutter` is already the Rust-side surface
//! Flutter apps depend on for the client runtime, so it is also the
//! source crate `flutter_rust_bridge_codegen` runs against to produce the
//! published `cratestack_cbor` Dart package (maintainer decision,
//! cratestack#563).
//!
//! Contains no CBOR wire-format logic of its own — [`encode_value`]/
//! [`decode_bytes`] call straight into `CborCodec`; see
//! [`mod@json_value`] for the one deliberate translation this boundary
//! needs (JSON `null` -> CBOR null).
//!
//! **Why JSON text, not a native Dart value type, crosses the frb
//! boundary:** unlike napi (which hands `encode`/`decode` a real JS
//! `Value` via napi's `serde-json` feature) or wasm-bindgen (which has
//! `JsValue`), flutter_rust_bridge has no dynamic "any JSON value" wire
//! type — every bridged function signature is a concrete, statically
//! known Rust type its codegen can walk ahead of time. [`encode_json`]/
//! [`decode_json`] therefore take/return a JSON-encoded `String`; a Dart
//! caller runs `jsonEncode`/`jsonDecode` (both in `dart:convert`, no
//! extra package) on their side. This costs one JSON stringify/parse per
//! call that napi/wasm's direct object marshaling doesn't pay — see
//! `benches/cbor_bridge/README.md` for the measured end-to-end numbers
//! against `package:cbor` (JSON stringify overhead included).
//!
//! **Relationship to [`crate::FlutterCborSeqDecoder`]:** that type solves
//! a different problem — finding item *boundaries* in a streamed
//! `application/cbor-seq` body — and its own doc comment used to tell
//! callers to decode each item's bytes with "any pure-Dart CBOR package".
//! [`decode_json`] is now that recommendation: the two compose
//! (`FlutterCborSeqDecoder::feed` finds the item bytes, `decode_json`
//! decodes them) instead of overlapping — neither decodes a value the
//! other already handles.
//!
//! **A real limitation, not just a style choice:** because the boundary
//! type is JSON, this bridge cannot *write* a CBOR byte string. A native
//! `uuid::Uuid`-typed field takes a compact 16-byte binary CBOR form
//! (`Serializer::is_human_readable() == false`) that a JSON-text-mediated
//! value can never produce — `serde_json` always reports
//! `is_human_readable() == true`, so a `Uuid` the Dart client holds as a
//! string goes out as a CBOR *text* string (it does not affect `Decimal`,
//! which already serializes as a string unconditionally).
//! `crates/cratestack-client-flutter/tests/cbor_bridge.rs` pins this.
//!
//! cratestack#1359 resolved the two halves differently. **Encode** stays
//! text-only, and the *server* now accepts a `Uuid` as text or 16 bytes
//! (`cratestack_core::lenient_uuid`). **Decode** used to refuse a byte
//! string outright (`serde_json::Value` has no such arm), failing the whole
//! body for one `Uuid`; it now writes a byte string as a JSON array of
//! integers, which the generated Dart `fromWire` turns back into the
//! canonical string (`cratestackDecodeUuid`).
//!
//! Split from a single file into [`mod@json_value`] per this workspace's
//! ~200-LoC file-size convention (mirrors `cratestack-cbor-napi`'s own
//! `lib.rs`/`json_value.rs` split).

pub mod compact;
mod json_value;

use serde_json::Value;

use cratestack_client_rust::RuntimeErrorCode;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{CratestackCodec, CratestackError};

use crate::types::FlutterRuntimeError;
use json_value::EncodableValue;

// `#[frb(sync)]` is applied unconditionally to `encode_json`/`decode_json`
// below, not just in the frb-generated glue. When `flutter_rust_bridge_codegen`
// runs against this crate (the `frb-glue` feature, `flutter_rust_bridge.yaml`),
// `#[frb]` is a passthrough attribute macro (verified against
// `flutter_rust_bridge_macros` 2.12.0's source: for any keyword other than
// `external`/`ui_state` it re-emits the item unchanged plus an encoded doc
// comment codegen reads) — so it compiles to a no-op here regardless of
// whether `flutter_rust_bridge` is even a dependency of this build, and the
// `#[cfg_attr]` below only pulls the crate in when `frb-glue` is enabled.
// `sync` matters, not just "some annotation": `benches/cbor_bridge/README.md`
// measured frb's *default* async dispatch at 0.5x pure-Dart `package:cbor` —
// a regression, not just underperformance — purely from per-call async port
// overhead. `#[frb(sync)]` is what the same benchmark's ~3-4.4x numbers
// actually used.

/// `"application/cbor"`, mirrored from `CborCodec::CONTENT_TYPE` — exposed
/// as a function (matching the napi/wasm siblings) so the value has one
/// source of truth instead of being copy-pasted into generated Dart.
///
/// `#[frb(ignore)]`: `&'static str` has no frb-compatible return
/// representation (verified directly — without this, `flutter_rust_bridge_codegen`
/// logs `Output type of \`content_type\` is a reference, thus currently set
/// to unit type` and generates a `Future<void> contentType()` stub that
/// silently discards the actual string, which is worse than not bridging
/// it at all). Skipped from the Dart surface for this slice; napi/wasm can
/// return it directly because their FFI boundaries support owned string
/// values without frb's static-type-per-call-signature constraint (see
/// `src/cbor/mod.rs`'s module doc for the same constraint's effect on
/// `encode_json`/`decode_json`'s `String`-typed boundary).
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
pub fn content_type() -> &'static str {
    CborCodec::CONTENT_TYPE
}

fn to_flutter_error(code: RuntimeErrorCode, error: impl ToString) -> FlutterRuntimeError {
    FlutterRuntimeError {
        code: code as u32,
        http_status: None,
        message: error.to_string(),
        remote_code: None,
        remote_body: None,
    }
}

/// Encodes a JSON value to CBOR bytes via `CborCodec`, through
/// [`EncodableValue`]. No `flutter_rust_bridge` types in the signature —
/// same reasoning as `cratestack-cbor-napi`'s `encode_value`/
/// `decode_bytes` split: it keeps this logic directly unit-testable
/// (see the tests below) independent of whether frb-generated glue is
/// present, which it deliberately is not in this checkout (cratestack#563
/// decision: glue is generated in CI, not committed).
pub(crate) fn encode_value(value: &Value) -> Result<Vec<u8>, CratestackError> {
    CborCodec.encode(&EncodableValue(value))
}

/// Decodes CBOR bytes to a JSON value via `CborCodec`.
///
/// Decodes through [`RpcFrameValue`] — `serde_json::Value`'s visitor plus a
/// byte-string arm — not `serde_json::Value` itself, which has no byte-string
/// arm and so refused the whole body for one `Uuid` (cratestack#1359: the
/// server writes a `Uuid` as a 16-byte CBOR byte string). A byte string comes
/// out as a JSON array of integers; everything `serde_json::Value` could
/// decode comes out identically (`tests::frame_value_decode_matches_the_legacy_json_value_decode`).
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<Value, CratestackError> {
    CborCodec.decode::<RpcFrameValue>(bytes).map(Value::from)
}

/// flutter_rust_bridge entry point: JSON text -> CBOR bytes. See the
/// module doc comment for why the boundary type is `String`, not a
/// native dynamic value.
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
pub fn encode_json(json: String) -> Result<Vec<u8>, FlutterRuntimeError> {
    let value: Value = serde_json::from_str(&json)
        .map_err(|error| to_flutter_error(RuntimeErrorCode::BadInput, error))?;
    encode_value(&value).map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))
}

/// flutter_rust_bridge entry point: CBOR bytes -> JSON text. Malformed
/// input returns a catchable `Err`, matching the napi/wasm siblings'
/// "never a panic on bad input" contract.
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
pub fn decode_json(bytes: Vec<u8>) -> Result<String, FlutterRuntimeError> {
    let value =
        decode_bytes(&bytes).map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))?;
    serde_json::to_string(&value).map_err(|error| to_flutter_error(RuntimeErrorCode::Codec, error))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_byte_strings;
