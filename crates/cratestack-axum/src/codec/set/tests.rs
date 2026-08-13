//! cratestack#489: `CodecSet::can_encode` must agree with what
//! `encode_response`/`encode_sequence_response` etc. actually do,
//! including the `application/cbor-seq` special case (encodable
//! whenever *either* slot is a CBOR codec, regardless of position).

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

use super::*;
use crate::transport::{CBOR_SEQUENCE_CONTENT_TYPE, HttpTransport};

#[test]
fn reports_both_configured_codecs_as_encodable() {
    let codec = CodecSet::new(CborCodec, JsonCodec);
    assert!(codec.can_encode("application/cbor"));
    assert!(codec.can_encode("application/json"));
}

#[test]
fn does_not_claim_a_codec_it_was_not_built_with() {
    let codec = CodecSet::new(CborCodec, JsonCodec);
    assert!(!codec.can_encode("text/plain"));
}

#[test]
fn cbor_seq_is_encodable_when_cbor_is_primary() {
    let codec = CodecSet::new(CborCodec, JsonCodec);
    assert!(codec.can_encode(CBOR_SEQUENCE_CONTENT_TYPE));
}

#[test]
fn cbor_seq_is_encodable_when_cbor_is_secondary() {
    let codec = CodecSet::new(JsonCodec, CborCodec);
    assert!(codec.can_encode(CBOR_SEQUENCE_CONTENT_TYPE));
}

#[test]
fn cbor_seq_is_not_encodable_without_any_cbor_codec() {
    // Not a real-world configuration (both slots are `JsonCodec`), but
    // exercises the "neither slot is CBOR" branch `can_encode` must
    // still get right rather than default to `true`.
    let codec = CodecSet::new(JsonCodec, JsonCodec);
    assert!(!codec.can_encode(CBOR_SEQUENCE_CONTENT_TYPE));
}
