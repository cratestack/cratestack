use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use cratestack_core::{CoolError, RouteTransportCapabilities};
use serde::Serialize;

use super::http_transport::HttpTransport;
use super::internal::fallback_error_response;
use super::media_type::select_response_content_type;

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
