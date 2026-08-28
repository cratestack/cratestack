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
use cratestack_core::{CratestackCodec, Value};
use wasm_bindgen::prelude::*;

use crate::value_bridge::JsSerializable;

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
/// `value` is deserialized into an owned `cratestack_core::Value` first
/// (JS has no notion of the concrete Rust types `CborCodec::encode<T>` is
/// generic over), then encoded. `Value` rather than `serde_json::Value`
/// is what makes binary data work at all: a JS `Uint8Array`/`ArrayBuffer`
/// lands in `Value::Bytes` (`serde-wasm-bindgen`'s `deserialize_any`
/// routes both to `visit_byte_buf`) and goes out as a CBOR byte string,
/// instead of degrading into a map of index→value that no `Vec<u8>` on
/// the Rust side can decode — cratestack#783. See `value_bridge.rs` for
/// the full rationale and the host-runnable byte assertions.
#[wasm_bindgen]
pub fn encode(value: JsValue) -> Result<Vec<u8>, JsError> {
    let value: Value = serde_wasm_bindgen::from_value(value)
        .map_err(|error| JsError::new(&format!("invalid value for CBOR encode: {error}")))?;
    let bytes = CborCodec.encode(&value)?;
    Ok(bytes)
}

/// Decodes CBOR bytes into a plain JS value. A CBOR byte string comes
/// back as a `Uint8Array` — the inverse of [`encode`]'s handling, closing
/// cratestack#783's symmetric decode half. Two things are load-bearing
/// for that: `JsSerializable` (which pins `Value::Bytes` to
/// `serialize_bytes` rather than the base64 string `Value` emits for
/// human-readable formats, and `serde_wasm_bindgen::Serializer` reports
/// itself as one), and leaving `serialize_bytes_as_arrays` at its default
/// so `serialize_bytes` produces a `Uint8Array` and not a plain `Array`.
/// Malformed input surfaces as a rejected/thrown `Error`, never a trap —
/// see the module doc comment.
///
/// Two `serde_wasm_bindgen` default `Serializer` behaviors would otherwise
/// corrupt the result, so this builds an explicit `Serializer` instead of
/// using the `to_value` convenience function:
///   - `serialize_maps_as_objects(true)` — the default turns Rust maps
///     (including `Value::Map`) into JS `Map` instances, not plain `{}`
///     objects. A `Map` round-trips fine through explicit
///     `.get()`/`.entries()` calls, but silently renders as `"{}"` under
///     `JSON.stringify`, which isn't what a `decode(bytes): unknown`
///     caller expects.
///   - `serialize_missing_as_null(true)` — the default serializes both
///     `serialize_none()` (what `Value::Null` calls) and
///     `serialize_unit()` as JS `undefined`, not `null`. An object
///     property set to `undefined` is silently dropped by
///     `JSON.stringify`, which is what made this look like data loss
///     during development rather than a `null`-vs-`undefined` mismatch.
#[wasm_bindgen]
pub fn decode(bytes: &[u8]) -> Result<JsValue, JsError> {
    let value: Value = CborCodec.decode(bytes)?;
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    serde::Serialize::serialize(&JsSerializable(&value), &serializer)
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
    fn a_uint8_array_encodes_as_a_cbor_byte_string() {
        // cratestack#783: this used to encode as a CBOR *map* of
        // index→value (`a8 6130 01 6131 02 …`), which no `Vec<u8>` on the
        // Rust side can decode. `0x44` is major type 2, length 4.
        let input = js_sys::Uint8Array::from(&[1u8, 2, 3, 4][..]);
        let bytes = encode(input.into()).expect("encode should succeed");
        assert_eq!(bytes, vec![0x44, 0x01, 0x02, 0x03, 0x04]);
    }

    #[wasm_bindgen_test]
    fn an_array_buffer_encodes_as_a_cbor_byte_string_too() {
        let view = js_sys::Uint8Array::from(&[1u8, 2, 3, 4][..]);
        let bytes = encode(view.buffer().into()).expect("encode should succeed");
        assert_eq!(bytes, vec![0x44, 0x01, 0x02, 0x03, 0x04]);
    }

    #[wasm_bindgen_test]
    fn a_cbor_byte_string_decodes_back_to_a_uint8_array() {
        // The symmetric half: a server sending a `Bytes` field as major
        // type 2 must be readable here, not just writable.
        let decoded = decode(&[0x44, 0x01, 0x02, 0x03, 0x04]).expect("decode should succeed");
        let array = decoded
            .dyn_ref::<js_sys::Uint8Array>()
            .expect("a CBOR byte string must decode to a Uint8Array");
        assert_eq!(array.to_vec(), vec![1, 2, 3, 4]);
    }

    #[wasm_bindgen_test]
    fn a_nested_uint8_array_round_trips() {
        let input = js_sys::Object::new();
        js_sys::Reflect::set(
            &input,
            &JsValue::from_str("nonce"),
            &js_sys::Uint8Array::from(&[0xdeu8, 0xad, 0xbe, 0xef][..]),
        )
        .expect("set should succeed");

        let bytes = encode(input.into()).expect("encode should succeed");
        let nonce = js_sys::Reflect::get(
            &decode(&bytes).expect("decode should succeed"),
            &JsValue::from_str("nonce"),
        )
        .expect("nonce property should exist");

        assert_eq!(
            nonce
                .dyn_ref::<js_sys::Uint8Array>()
                .expect("nonce must come back as a Uint8Array")
                .to_vec(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
    }

    #[wasm_bindgen_test]
    fn a_plain_number_array_is_still_a_plain_array() {
        // The `Array.from(bytes)` workaround callers use today must keep
        // behaving exactly as before — an untyped value carries no
        // schema, so nothing here may guess that an integer array "meant"
        // bytes. `0x84` is a 4-element array, not `0x44`.
        let input = js_sys::Array::of4(
            &JsValue::from_f64(1.0),
            &JsValue::from_f64(2.0),
            &JsValue::from_f64(3.0),
            &JsValue::from_f64(4.0),
        );
        let bytes = encode(input.into()).expect("encode should succeed");
        assert_eq!(bytes, vec![0x84, 0x01, 0x02, 0x03, 0x04]);

        let decoded = decode(&bytes).expect("decode should succeed");
        assert!(
            js_sys::Array::is_array(&decoded),
            "an integer array must decode back as an Array, not a Uint8Array"
        );
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
