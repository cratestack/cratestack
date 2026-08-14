use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

use crate::transport::fallback_error_response;

pub fn decode_codec_request<C, T>(codec: &C, body: &[u8]) -> Result<T, CratestackError>
where
    C: CratestackCodec,
    T: for<'de> Deserialize<'de>,
{
    codec.decode(body)
}

pub fn encode_codec_response<C, T>(
    codec: &C,
    status: StatusCode,
    value: &T,
) -> Result<Response, CratestackError>
where
    C: CratestackCodec,
    T: Serialize + ?Sized,
{
    let bytes = codec.encode(value)?;
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(C::CONTENT_TYPE),
    );
    Ok(response)
}

pub fn encode_codec_result<C, T>(codec: &C, result: Result<T, CratestackError>) -> Response
where
    C: CratestackCodec,
    T: Serialize,
{
    encode_codec_result_with_status(codec, StatusCode::OK, result)
}

pub fn encode_codec_result_with_status<C, T>(
    codec: &C,
    success_status: StatusCode,
    result: Result<T, CratestackError>,
) -> Response
where
    C: CratestackCodec,
    T: Serialize,
{
    match result {
        Ok(value) => encode_codec_response(codec, success_status, &value)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            encode_codec_response(codec, status, &body).unwrap_or_else(fallback_error_response)
        }
    }
}
