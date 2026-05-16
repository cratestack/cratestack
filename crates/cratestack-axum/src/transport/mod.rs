//! Transport-level abstractions: the [`HttpTransport`] trait, transport
//! header validation, the `encode_transport_*` and
//! `encode_transport_sequence_*` response-encoding families, and the
//! shared media-type helpers they rely on.

mod encode_sequence;
mod encode_unary;
mod http_transport;
mod internal;
mod media_type;
mod validate;

pub const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

pub use encode_sequence::{
    encode_transport_sequence_result, encode_transport_sequence_result_with_status,
    encode_transport_sequence_result_with_status_for,
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
