use axum::http::HeaderMap;
use cratestack_core::{CoolError, RouteTransportCapabilities};
use serde::Deserialize;

use super::http_transport::HttpTransport;
use super::media_type::{
    request_content_type, validate_transport_accept_header, validate_transport_content_type_header,
};

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
