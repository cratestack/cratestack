//! Transport-level abstractions: the [`HttpTransport`] trait, transport
//! header validation, the `encode_transport_*` and
//! `encode_transport_sequence_*` response-encoding families, and the
//! shared media-type helpers they rely on.

mod encode_sequence;
mod encode_unary;
mod http_transport;
mod internal;
mod media_type;
mod stream_sequence;
mod validate;

pub const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

/// Response header set only by genuinely incrementally-streamed
/// `application/cbor-seq` responses (`@stream` procedures, via
/// [`stream_sequence::encode_cbor_sequence_stream_response`] —
/// cratestack#283). Not part of the documented wire contract; it's a
/// server-internal signal so response-buffering middleware (currently
/// just [`crate::idempotency::IdempotencyService`]) can distinguish a
/// truly incremental body from an ordinary buffered one and bypass
/// buffering instead of silently re-collecting a partial stream. See
/// `crate::idempotency::service` for the consumer.
pub(crate) const STREAM_RESPONSE_HEADER: &str = "x-cratestack-stream";

pub use encode_sequence::{
    encode_transport_sequence_result, encode_transport_sequence_result_with_status,
    encode_transport_sequence_result_with_status_for,
    encode_transport_stream_result_with_status_for,
};
pub use encode_unary::{
    encode_transport_result, encode_transport_result_with_status,
    encode_transport_result_with_status_for,
};
pub use http_transport::HttpTransport;
pub use validate::{
    decode_transport_request_for, validate_transport_request_headers,
    validate_transport_request_headers_for, validate_transport_response_headers,
    validate_transport_response_headers_for,
};

pub(crate) use http_transport::CborCodecMarker;
pub(crate) use internal::{encode_cbor_sequence_response, fallback_error_response};
pub(crate) use media_type::{
    validate_transport_accept_header, validate_transport_content_type_header,
};
pub(crate) use stream_sequence::encode_cbor_sequence_stream_response;
