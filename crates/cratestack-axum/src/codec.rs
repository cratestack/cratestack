//! Codec-bound helpers: request/response header validation and
//! encode/decode for a single `CoolCodec`, plus the [`CodecSet`] pairing
//! that lets a router serve two codecs (e.g. CBOR + JSON) from the same
//! endpoint.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError, CoolErrorResponse};
use serde::{Deserialize, Serialize};

use crate::transport::{
    CBOR_SEQUENCE_CONTENT_TYPE, CborCodecMarker, HttpTransport, encode_cbor_sequence_response,
    fallback_error_response, validate_transport_accept_header,
    validate_transport_content_type_header,
};

#[derive(Debug, Clone)]
pub struct CodecSet<Primary, Secondary> {
    primary: Primary,
    secondary: Secondary,
}

impl<Primary, Secondary> CodecSet<Primary, Secondary> {
    pub fn new(primary: Primary, secondary: Secondary) -> Self {
        Self { primary, secondary }
    }
}

impl<Primary, Secondary> HttpTransport for CodecSet<Primary, Secondary>
where
    Primary: CoolCodec,
    Secondary: CoolCodec,
{
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if content_type == Primary::CONTENT_TYPE {
            self.primary.decode(body)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.secondary.decode(body)
        } else {
            Err(CoolError::UnsupportedMediaType(format!(
                "unsupported request Content-Type {content_type}"
            )))
        }
    }

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized,
    {
        if content_type == Primary::CONTENT_TYPE {
            encode_codec_response(&self.primary, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            encode_codec_response(&self.secondary, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, values)
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, values)
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, std::slice::from_ref(value))
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, std::slice::from_ref(value))
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }
}

pub fn validate_codec_response_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)
}

pub fn validate_codec_request_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)?;
    validate_content_type_header::<C>(headers)
}

pub(crate) fn validate_accept_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_accept_header(headers, &[C::CONTENT_TYPE])
}

pub(crate) fn validate_content_type_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_content_type_header(headers, &[C::CONTENT_TYPE])
}

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
