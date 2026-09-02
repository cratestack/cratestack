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

/// Marker inserted into `Response::extensions()` (never `headers()`) by
/// genuinely incrementally-streamed `application/cbor-seq` responses
/// (`@stream` procedures, via
/// [`stream_sequence::encode_cbor_sequence_stream_response`] —
/// cratestack#283), so response-buffering middleware (currently just
/// [`crate::idempotency::IdempotencyService`]) can distinguish a truly
/// incremental body from an ordinary buffered one and bypass buffering
/// instead of silently re-collecting a partial stream. See
/// `crate::idempotency::service` for the consumer.
///
/// Deliberately an extension, not a header: an extension is an in-process
/// `http::Extensions` type-map entry on the `Response` struct — it has no
/// wire representation at all, by construction, so there is no code path
/// (missed strip step, a route with no idempotency middleware in front of
/// it, ...) through which this could ever reach a real client. A header
/// carrying the same "internal-only" intent would need every exit path
/// to remember to strip it; this needs none of them to.
#[derive(Clone, Copy)]
pub(crate) struct StreamedResponseMarker;

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
    select_transport_response_content_type, validate_transport_accept_header,
    validate_transport_content_type_header,
};
pub(crate) use stream_sequence::encode_cbor_sequence_stream_response;
