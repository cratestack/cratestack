//! `wasm-bindgen` bindings exposed to `packages/cratestack-cbor-web`'s
//! `createCborCodec()` factory. Everything here is a thin shim over
//! `cratestack-codec-cbor::CborCodec` — no CBOR encode/decode logic lives
//! in this crate, only the JsValue <-> Rust plumbing.
//!
//! Every fallible export returns `Result<_, JsError>` rather than
//! panicking/unwrapping. This matters specifically for `decode`: a wasm
//! trap (from a panic reaching the wasm/JS boundary) leaves the module's
//! linear memory in an unspecified state for every *subsequent* call too,
//! not just the failing one, so malformed input must produce a catchable
//! rejected-promise-shaped `Result::Err`, never a trap. `JsError`
//! implements `From<E: std::error::Error>`, and `CratestackError` (thiserror)
//! qualifies, so `?` converts codec errors into a real JS `Error` object
//! (readable `.message`, `instanceof Error`) instead of a bare string.
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use wasm_bindgen::prelude::*;

use crate::json_bridge::EncodableValue;

/// Runs once when the wasm module is instantiated (part of the generated
/// `init()` glue) — before this, any panic elsewhere in the module would
/// surface as an opaque "unreachable executed" trap in the browser
/// console; after, it's a readable stack trace.
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// Mirrors `CborCodec::CONTENT_TYPE` ("application/cbor") — kept as a
/// function rather than a duplicated JS-side string literal so there is
/// exactly one source of truth for the value.
#[wasm_bindgen(js_name = contentType)]
pub fn content_type() -> String {
    CborCodec::CONTENT_TYPE.to_string()
}

/// Encodes an arbitrary JS value as CBOR bytes.
///
/// `value` is deserialized into an owned `serde_json::Value` first (JS has
/// no notion of the concrete Rust types `CborCodec::encode<T>` is generic
/// over), then re-serialized through `EncodableValue` — see
/// `json_bridge.rs` for why that indirection exists rather than encoding
/// the `serde_json::Value` tree directly.
#[wasm_bindgen]
pub fn encode(value: JsValue) -> Result<Vec<u8>, JsError> {
    let value: serde_json::Value = serde_wasm_bindgen::from_value(value)
        .map_err(|error| JsError::new(&format!("invalid value for CBOR encode: {error}")))?;
    let bytes = CborCodec.encode(&EncodableValue(&value))?;
    Ok(bytes)
}

/// Decodes CBOR bytes into a plain JS value. Malformed input surfaces as
/// a rejected/thrown `Error`, never a trap — see the module doc comment.
///
/// Two `serde_wasm_bindgen` default `Serializer` behaviors would otherwise
/// corrupt the result, so this builds an explicit `Serializer` instead of
/// using the `to_value` convenience function:
///   - `serialize_maps_as_objects(true)` — the default turns Rust maps
///     (including `serde_json::Value::Object`) into JS `Map` instances,
///     not plain `{}` objects. A `Map` round-trips fine through explicit
///     `.get()`/`.entries()` calls, but silently renders as `"{}"` under
///     `JSON.stringify`, which isn't what a `decode(bytes): unknown`
///     caller expects.
///   - `serialize_missing_as_null(true)` — the default serializes both
///     `serialize_none()` and `serialize_unit()` (what `serde_json::Value
///     ::Null`'s own `Serialize` impl calls) as JS `undefined`, not
///     `null`. An object property set to `undefined` is silently dropped
///     by `JSON.stringify`, which is what made this look like data loss
///     during development rather than a `null`-vs-`undefined` mismatch.
#[wasm_bindgen]
pub fn decode(bytes: &[u8]) -> Result<JsValue, JsError> {
    let value: serde_json::Value = CborCodec.decode(bytes)?;
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    serde::Serialize::serialize(&value, &serializer)
        .map_err(|error| JsError::new(&format!("failed to convert decoded CBOR to JS: {error}")))
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn content_type_matches_the_codec() {
        assert_eq!(content_type(), "application/cbor");
    }

    #[wasm_bindgen_test]
    fn round_trips_a_plain_object() {
        let json = r#"{"name":"cratestack","tags":["cool","stack"],"count":2,"note":null}"#;
        let input = js_sys::JSON::parse(json).expect("valid JSON literal");
        let bytes = encode(input).expect("encode should succeed");
        let decoded = decode(&bytes).expect("decode should succeed");

        // Compare structurally (not via JSON.stringify) since
        // serde_json::Value's default map has no defined key order.
        let decoded: serde_json::Value =
            serde_wasm_bindgen::from_value(decoded).expect("decoded value should deserialize");
        let expected: serde_json::Value = serde_json::from_str(json).expect("valid JSON literal");
        assert_eq!(decoded, expected);
    }

    #[wasm_bindgen_test]
    fn null_field_round_trips_as_real_js_null_not_undefined() {
        let bytes = encode(js_sys::JSON::parse(r#"{"note":null}"#).expect("valid JSON"))
            .expect("encode should succeed");
        let decoded = decode(&bytes).expect("decode should succeed");
        let note = js_sys::Reflect::get(&decoded, &JsValue::from_str("note"))
            .expect("note property should exist");
        assert!(note.is_null(), "expected JS null, got {note:?}");
        assert!(!note.is_undefined(), "note must not be undefined");
        // JSON.stringify keeps `null`-valued keys but drops
        // `undefined`-valued ones — a stronger, more legible way to prove
        // the property survived as `null` and not `undefined`.
        let stringified = js_sys::JSON::stringify(&decoded)
            .expect("stringify should succeed")
            .as_string()
            .expect("stringify returns a JS string");
        assert_eq!(stringified, r#"{"note":null}"#);
    }

    #[wasm_bindgen_test]
    fn top_level_null_encodes_to_the_single_cbor_null_byte() {
        let bytes = encode(JsValue::NULL).expect("encode should succeed");
        assert_eq!(bytes, vec![0xf6]);
    }

    #[wasm_bindgen_test]
    fn malformed_cbor_rejects_with_a_catchable_error_not_a_trap() {
        let malformed = [0xff, 0x00, 0x01];
        let result = decode(&malformed);
        assert!(
            result.is_err(),
            "malformed CBOR must not decode successfully"
        );
        // The module must still be usable afterwards — a trap would have
        // poisoned it. Proving a normal call still works after the error
        // is the actual regression test for "not a trap".
        assert_eq!(content_type(), "application/cbor");
        let ok = encode(JsValue::from_f64(1.0)).expect("module must still work after an error");
        assert!(!ok.is_empty());
    }
}
