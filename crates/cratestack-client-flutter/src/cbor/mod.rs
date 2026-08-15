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
//! type is JSON, this bridge cannot reproduce a scalar encoding that
//! depends on `Serializer::is_human_readable() == false` — `CborCodec`
//! encoding a native `uuid::Uuid`-typed field directly takes a compact
//! 16-byte binary CBOR branch that a JSON-text-mediated value can never
//! produce (JSON has no binary primitive); `serde_json` always reports
//! `is_human_readable() == true`, so the value takes the hex-string
//! branch instead. `crates/cratestack-client-flutter/tests/cbor_bridge.rs`
//! documents this precisely (it does not affect `Decimal`, which already
//! serializes as a string unconditionally). Worth resolving explicitly —
//! not inheriting silently — when the Dart generator seam (out of scope
//! for this PR) decides how `Uuid`/`Cuid` fields cross this bridge.
//!
//! Split from a single file into [`mod@json_value`] per this workspace's
//! ~200-LoC file-size convention (mirrors `cratestack-cbor-napi`'s own
//! `lib.rs`/`json_value.rs` split).

mod json_value;

use serde_json::Value;

use cratestack_client_rust::RuntimeErrorCode;
use cratestack_codec_cbor::CborCodec;
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
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<Value, CratestackError> {
    CborCodec.decode(bytes)
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn content_type_matches_codec_constant() {
        assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
        assert_eq!(content_type(), CborCodec::CONTENT_TYPE);
    }

    #[test]
    fn encode_then_decode_round_trips_through_the_json_text_wrapper_functions() {
        // Exercises the exact logic frb's `encode_json`/`decode_json`
        // would delegate to once codegen wraps them (not just
        // `CborCodec` in isolation).
        let json = r#"{"cratestack":["cool","stack"],"n":42}"#;
        let bytes = encode_json(json.to_owned()).expect("encode should succeed");
        let decoded_json = decode_json(bytes).expect("decode should succeed");
        let decoded: Value = serde_json::from_str(&decoded_json).unwrap();
        assert_eq!(decoded, json!({"cratestack": ["cool", "stack"], "n": 42}));
    }

    #[test]
    fn top_level_none_round_trips_as_cbor_null() {
        // Proves this wrapper preserves cratestack-codec-cbor's
        // documented `Option::None` -> CBOR null (`0xf6`) behavior, not
        // the `Value::Null`-via-`serialize_unit` empty-array (`0x80`)
        // quirk its own test suite warns about.
        let bytes = encode_value(&Value::Null).expect("encode null should succeed");
        assert_eq!(bytes, vec![0xf6]);
        let decoded = decode_bytes(&bytes).expect("decode should succeed");
        assert_eq!(decoded, Value::Null);
    }

    #[test]
    fn malformed_cbor_bytes_return_an_error_not_a_panic() {
        // 0x1b announces an 8-byte unsigned integer but supplies none —
        // truncated/malformed input `minicbor` rejects with an error
        // rather than panicking.
        let malformed = vec![0x1b];
        let result = decode_json(malformed);
        assert!(result.is_err(), "malformed CBOR must error, not panic");
    }

    #[test]
    fn invalid_json_text_returns_an_error_not_a_panic() {
        let result = encode_json("{not valid json".to_owned());
        assert!(
            result.is_err(),
            "malformed JSON input must error, not panic"
        );
    }

    #[test]
    fn fixture_bytes_shared_with_the_napi_and_wasm_cross_language_tests_stay_correct() {
        // Same exact hex fixtures as `cratestack-cbor-napi`'s
        // `fixture_bytes_shared_with_the_js_cross_language_test_stay_correct`
        // (`crates/cratestack-cbor-napi/src/lib.rs`) — proving this
        // bridge produces byte-identical CBOR to the Node addon (and,
        // transitively, the wasm build, which shares the same fixtures)
        // for the same JSON input. Three independent bindings asserting
        // the same wire bytes from three ends of the FFI boundary is what
        // "byte-identical to the Rust CborCodec" actually proves.
        assert_eq!(
            hex(&encode_value(&json!(["cool", "stack"])).expect("encode")),
            "8264636f6f6c65737461636b"
        );
        assert_eq!(
            hex(
                &encode_value(&json!({"cratestack": ["cool", "stack"], "n": 42, "ok": true}))
                    .expect("encode")
            ),
            "a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5"
        );
        assert_eq!(
            hex(&encode_value(&json!({"a": null, "b": [1, null, "x"]})).expect("encode")),
            "a26161f661628301f66178"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
