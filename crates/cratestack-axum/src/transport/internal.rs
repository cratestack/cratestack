use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError};
use serde::Serialize;

use super::http_transport::CborCodecMarker;
use super::CBOR_SEQUENCE_CONTENT_TYPE;

pub(crate) fn encode_cbor_sequence_response<C, T>(
    codec: &C,
    status: StatusCode,
    values: &[T],
) -> Result<Response, CoolError>
where
    C: CoolCodec,
    T: Serialize,
{
    if C::CONTENT_TYPE != CborCodecMarker::CONTENT_TYPE {
        return Err(CoolError::NotAcceptable(
            "cbor-seq requires a CBOR codec".to_owned(),
        ));
    }

    let mut bytes = Vec::new();
    for value in values {
        bytes.extend(codec.encode(value)?);
    }
    encode_bytes_response(status, CBOR_SEQUENCE_CONTENT_TYPE, bytes)
}

pub(crate) fn encode_bytes_response(
    status: StatusCode,
    content_type: &'static str,
    bytes: Vec<u8>,
) -> Result<Response, CoolError> {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    Ok(response)
}

pub(crate) fn fallback_error_response(error: CoolError) -> Response {
    let mut response = Response::new(Body::from(error.public_message().into_owned()));
    *response.status_mut() = error.status_code();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
