//! Codec-bound helpers: request/response header validation and
//! encode/decode for a single `CoolCodec`, plus the [`CodecSet`] pairing
//! that lets a router serve two codecs (e.g. CBOR + JSON) from the same
//! endpoint.

mod encode;
mod headers;
mod set;

pub use encode::{
    decode_codec_request, encode_codec_response, encode_codec_result,
    encode_codec_result_with_status,
};
pub use headers::{validate_codec_request_headers, validate_codec_response_headers};
pub use set::CodecSet;
