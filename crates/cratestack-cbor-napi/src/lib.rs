//! Native N-API bindings exposing `cratestack-codec-cbor`'s `CborCodec`
//! (`crates/cratestack-codec-cbor`) to Node, for `@cratestack/cbor-node`
//! (issue #286, epic #285). This crate contains no CBOR wire-format logic
//! of its own — [`encode_value`]/[`decode_bytes`] call straight into
//! `CborCodec`; see [`json_value`] for the one deliberate translation this
//! boundary needs (JSON `null` -> CBOR null), which is JS/Rust boundary
//! translation, not a `CborCodec` concern.
//!
//! **Why the pure `encode_value`/`decode_bytes` split exists:** napi's
//! `Uint8Array`/`Error` FFI types (used by the `#[napi]`-decorated
//! functions in [`addon`] below) reference real `napi_*` C symbols —
//! `napi_create_string_utf8`, `napi_reference_unref`, etc. — that only
//! resolve when the compiled `.node` addon is `dlopen`'d by an actual
//! running Node process, which provides them. A bare `cargo test` binary
//! has no such runtime, so any *reachable* code path that constructs or
//! drops those types fails at link time (confirmed empirically — see the
//! PR description). Rust's linker dead-strips code nothing calls, so
//! keeping the actual encode/decode logic in ordinary functions with no
//! napi types in their signature — with `addon::encode`/`addon::decode`
//! as thin marshaling shells around them — lets `cargo test -p
//! cratestack-cbor-napi` exercise the real logic directly, while the FFI
//! shell itself is proven by the JS/vitest suite in
//! `packages/cratestack-cbor-node`, which genuinely loads the addon
//! inside Node.
//!
//! `#[napi(catch_unwind)]` on both `addon` entry points converts an
//! unexpected Rust panic (e.g. a latent bug surfacing deep in a dependency
//! on adversarial input) into a catchable JS exception instead of aborting
//! the whole Node process — napi-derive only wraps a function body in
//! `std::panic::catch_unwind` when this attribute is present; it is
//! opt-in, not the default. Malformed CBOR input on the *ordinary* error
//! path never needs to reach that: `minicbor-serde`/`CborCodec::decode`
//! already return `Result`, and `#[napi]` functions returning
//! `napi::Result<T>` propagate `Err` as a normal, catchable JS exception
//! on their own — `catch_unwind` is defense in depth for panics
//! specifically, not what turns `Result::Err` into a JS exception (that
//! part just works, verified by this crate's own tests below).

mod json_value;

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use serde_json::Value;

use json_value::CborValue;

/// Encodes a JSON value to CBOR bytes via `CborCodec`. No napi types in
/// the signature — see the module docs for why that matters for testing.
fn encode_value(value: &Value) -> Result<Vec<u8>, CratestackError> {
    CborCodec.encode(&CborValue(value))
}

/// Decodes CBOR bytes to a JSON value via `CborCodec`.
fn decode_bytes(bytes: &[u8]) -> Result<Value, CratestackError> {
    CborCodec.decode(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
mod addon {
    use napi::bindgen_prelude::Uint8Array;
    use napi_derive::napi;
    use serde_json::Value;

    use crate::{decode_bytes, encode_value};
    use cratestack_codec_cbor::CborCodec;
    use cratestack_core::{CratestackCodec, CratestackError};

    fn to_napi_error(error: CratestackError) -> napi::Error {
        napi::Error::new(napi::Status::GenericFailure, error.to_string())
    }

    /// `"application/cbor"`, mirrored from `CborCodec::CONTENT_TYPE`.
    /// Exposed as a function (rather than duplicating the literal on the
    /// TypeScript side) so the two can't drift.
    #[napi]
    pub fn content_type() -> &'static str {
        CborCodec::CONTENT_TYPE
    }

    /// Encodes an arbitrary JS value to CBOR bytes.
    #[napi(catch_unwind)]
    pub fn encode(value: Value) -> napi::Result<Uint8Array> {
        encode_value(&value)
            .map(Uint8Array::from)
            .map_err(to_napi_error)
    }

    /// Decodes CBOR bytes to a JS value. Malformed input returns a
    /// catchable `Error` — never a native crash.
    #[napi(catch_unwind)]
    pub fn decode(bytes: Uint8Array) -> napi::Result<Value> {
        decode_bytes(bytes.as_ref()).map_err(to_napi_error)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use addon::*;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn content_type_matches_codec_constant() {
        assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
    }

    #[test]
    fn encode_then_decode_round_trips_through_the_wrapper_functions_directly() {
        // Exercises the exact logic napi's `encode`/`decode` delegate to
        // (not just the underlying CborCodec in isolation), per the
        // ticket's "unit tests on the napi wrapper functions directly".
        let value = json!({"cratestack": ["cool", "stack"], "n": 42});
        let bytes = encode_value(&value).expect("encode should succeed");
        let decoded = decode_bytes(&bytes).expect("decode should succeed");
        assert_eq!(decoded, value);
    }

    #[test]
    fn top_level_none_round_trips_as_cbor_null() {
        // The single most important test per the ticket: proves this
        // wrapper preserves cratestack-codec-cbor's documented
        // `Option::None` -> CBOR null (`0xf6`) behavior, not the
        // `Value::Null`-via-`serialize_unit` empty-array (`0x80`) quirk
        // its own test suite warns about.
        let bytes = encode_value(&Value::Null).expect("encode null should succeed");
        assert_eq!(bytes, vec![0xf6]);
        let decoded = decode_bytes(&bytes).expect("decode should succeed");
        assert_eq!(decoded, Value::Null);
    }

    #[test]
    fn malformed_cbor_bytes_return_an_error_not_a_panic() {
        // 0x1b announces an 8-byte unsigned integer but supplies none —
        // truncated/malformed input `minicbor` rejects with an error
        // rather than panicking. `addon::decode` maps this `Err` through
        // `napi::Result`, which napi-rs propagates as a catchable JS
        // exception (see module docs); `#[napi(catch_unwind)]` covers the
        // panic case beyond this, which isn't reachable from `cargo test`
        // (no napi runtime here to catch into) and is instead covered by
        // the JS/vitest suite in packages/cratestack-cbor-node.
        let malformed = [0x1b];
        let result = decode_bytes(&malformed);
        assert!(result.is_err(), "malformed CBOR must error, not panic");
    }

    #[test]
    fn fixture_bytes_shared_with_the_js_cross_language_test_stay_correct() {
        // These exact hex fixtures are independently re-asserted in
        // packages/cratestack-cbor-node/tests/codec.test.ts's
        // "cross-language fixtures" suite — the JS side hardcodes the
        // same hex strings and checks the *compiled Node addon*
        // (encode_value/decode_bytes below, wrapped by real napi FFI)
        // produces/consumes them identically. Two tests independently
        // asserting the same wire bytes from two ends of the FFI
        // boundary is what "byte-identical to the Rust CborCodec" (not
        // just same-language round-trips) actually proves; if this test
        // ever needs to change, the JS one needs the matching update.
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
