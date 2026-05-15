//! Transport-level abstractions: the [`HttpTransport`] trait, transport
//! header validation, the `encode_transport_*` and
//! `encode_transport_sequence_*` response-encoding families, and the
//! shared media-type helpers they rely on.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError, CoolErrorResponse, RouteTransportCapabilities};
use serde::{Deserialize, Serialize};

use crate::codec::encode_codec_response;

pub const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

pub trait HttpTransport: Clone + Send + Sync + 'static {
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>;

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized;

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize;

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError>;
}

impl<C> HttpTransport for C
where
    C: CoolCodec,
{
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            crate::codec::decode_codec_request(self, body)
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
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            encode_codec_response(self, status, value)
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
            encode_cbor_sequence_response(self, status, values)
        } else {
            self.encode_response(content_type, status, values)
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_response(self, status, std::slice::from_ref(value))
        } else {
            self.encode_response(content_type, status, value)
        }
    }
}

pub(crate) struct CborCodecMarker;

impl CborCodecMarker {
    pub(crate) const CONTENT_TYPE: &'static str = "application/cbor";
}

pub fn validate_transport_request_headers<T>(
    transport: &T,
    headers: &HeaderMap,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_request_headers_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
    )
}

pub fn validate_transport_response_headers<T>(
    transport: &T,
    headers: &HeaderMap,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_response_headers_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
    )
}

pub fn validate_transport_request_headers_for<T>(
    _transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, capabilities.response_types)?;
    if capabilities.request_types.is_empty() {
        Ok(())
    } else {
        validate_transport_content_type_header(headers, capabilities.request_types)
    }
}

pub fn validate_transport_response_headers_for<T>(
    _transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, capabilities.response_types)
}

pub fn decode_transport_request_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    body: &[u8],
) -> Result<TValue, CoolError>
where
    TTransport: HttpTransport,
    TValue: for<'de> Deserialize<'de>,
{
    let content_type = request_content_type(headers, capabilities.request_types)?;
    transport.decode_request(content_type, body)
}

pub(crate) fn validate_transport_accept_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(());
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    if supported
        .iter()
        .any(|content_type| accepts_content_type(accept, content_type))
    {
        Ok(())
    } else {
        Err(CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            supported.join(", "),
        )))
    }
}

pub(crate) fn validate_transport_content_type_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    request_content_type(headers, supported).map(|_| ())
}

pub(crate) fn request_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<&'static str, CoolError> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Err(CoolError::UnsupportedMediaType(format!(
            "expected Content-Type one of {}",
            supported.join(", "),
        )));
    };
    let content_type = content_type
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Content-Type header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|expected| media_type_matches(content_type, expected))
        .ok_or_else(|| {
            CoolError::UnsupportedMediaType(format!(
                "expected Content-Type one of {}, got {}",
                supported.join(", "),
                content_type,
            ))
        })
}

pub(crate) fn select_response_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
    default: &'static str,
) -> Result<&'static str, CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(default);
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|content_type| accepts_content_type(accept, content_type))
        .ok_or_else(|| {
            CoolError::NotAcceptable(format!(
                "router only serves {} responses",
                supported.join(", "),
            ))
        })
}

pub(crate) fn accepts_content_type(accept: &str, expected: &str) -> bool {
    accept.split(',').map(str::trim).any(|value| {
        if value == "*/*" {
            return true;
        }
        let media_type = strip_media_type_params(value);
        media_type == expected
            || media_type == wildcard_media_type(expected)
            || media_type == "application/*"
    })
}

pub(crate) fn media_type_matches(candidate: &str, expected: &str) -> bool {
    strip_media_type_params(candidate) == expected
}

pub(crate) fn strip_media_type_params(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

pub(crate) fn wildcard_media_type(content_type: &str) -> &str {
    content_type
        .split_once('/')
        .map(|(prefix, _)| {
            if prefix == "application" {
                "application/*"
            } else {
                "*/*"
            }
        })
        .unwrap_or("*/*")
}

pub fn encode_transport_result<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    result: Result<TValue, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    encode_transport_result_with_status_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
        StatusCode::OK,
        result,
    )
}

pub fn encode_transport_result_with_status<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    success_status: StatusCode,
    result: Result<TValue, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    encode_transport_result_with_status_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
        success_status,
        result,
    )
}

pub fn encode_transport_result_with_status_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    success_status: StatusCode,
    result: Result<TValue, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    let content_type = match select_response_content_type(
        headers,
        capabilities.response_types,
        capabilities.default_response_type,
    ) {
        Ok(content_type) => content_type,
        Err(error) => return fallback_error_response(error),
    };
    match result {
        Ok(value) => transport
            .encode_response(content_type, success_status, &value)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            transport
                .encode_response(content_type, status, &body)
                .unwrap_or_else(fallback_error_response)
        }
    }
}

pub fn encode_transport_sequence_result<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    result: Result<Vec<TValue>, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    encode_transport_sequence_result_with_status_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
        StatusCode::OK,
        result,
    )
}

pub fn encode_transport_sequence_result_with_status<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    success_status: StatusCode,
    result: Result<Vec<TValue>, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    encode_transport_sequence_result_with_status_for(
        transport,
        headers,
        &RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
        success_status,
        result,
    )
}

pub fn encode_transport_sequence_result_with_status_for<TTransport, TValue>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    success_status: StatusCode,
    result: Result<Vec<TValue>, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize,
{
    if !capabilities.supports_sequence_response {
        return fallback_error_response(CoolError::Internal(
            "sequence response encoding requested for a route without sequence capability"
                .to_owned(),
        ));
    }
    let content_type = match select_response_content_type(
        headers,
        capabilities.response_types,
        capabilities.default_response_type,
    ) {
        Ok(content_type) => content_type,
        Err(error) => return fallback_error_response(error),
    };
    match result {
        Ok(values) => transport
            .encode_sequence_response(content_type, success_status, &values)
            .unwrap_or_else(fallback_error_response),
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            transport
                .encode_sequence_error_response(content_type, status, &body)
                .unwrap_or_else(fallback_error_response)
        }
    }
}

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
