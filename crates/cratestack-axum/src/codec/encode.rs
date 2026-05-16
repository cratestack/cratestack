use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError};
use serde::{Deserialize, Serialize};

use crate::transport::fallback_error_response;

pub fn decode_codec_request<C, T>(codec: &C, body: &[u8]) -> Result<T, CoolError>
where
    C: CoolCodec,
    T: for<'de> Deserialize<'de>,
{
    codec.decode(body)
}

pub fn encode_codec_response<C, T>(
    codec: &C,
    status: StatusCode,
    value: &T,
) -> Result<Response, CoolError>
where
    C: CoolCodec,
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

pub fn encode_codec_result<C, T>(codec: &C, result: Result<T, CoolError>) -> Response
where
    C: CoolCodec,
    T: Serialize,
{
    encode_codec_result_with_status(codec, StatusCode::OK, result)
}

pub fn encode_codec_result_with_status<C, T>(
    codec: &C,
    success_status: StatusCode,
    result: Result<T, CoolError>,
) -> Response
where
    C: CoolCodec,
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
