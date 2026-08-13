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
    transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, &encodable_response_types(transport, capabilities))?;
    if capabilities.request_types.is_empty() {
        Ok(())
    } else {
        validate_transport_content_type_header(headers, capabilities.request_types)
    }
}

pub fn validate_transport_response_headers_for<T>(
    transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<(), CoolError>
where
    T: HttpTransport,
{
    validate_transport_accept_header(headers, &encodable_response_types(transport, capabilities))
}

/// `capabilities.response_types` filtered down to what `transport` can
/// actually encode (cratestack#489): the `Accept` preflight below must
/// reject a request naming only content types the router will fail to
/// produce *before* any handler side effect (e.g. a model `create`'s DB
/// write) runs — not just eventually, once the response encoder in
/// `encode_transport_result_with_status_for` catches it too late for a
/// mutation that has already happened. Empty `response_types` (the
/// capability-free `validate_transport_request_headers`/
/// `validate_transport_response_headers` wrappers below, unused in this
/// workspace) passes through unfiltered — `validate_transport_accept_header`
/// already treats an empty list as "reject anything with an explicit
/// `Accept`", unchanged from before this fix.
fn encodable_response_types<T>(
    transport: &T,
    capabilities: &RouteTransportCapabilities,
) -> Vec<&'static str>
where
    T: HttpTransport,
{
    capabilities
        .response_types
        .iter()
        .copied()
        .filter(|content_type| transport.can_encode(content_type))
        .collect()
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

#[cfg(test)]
mod tests;
