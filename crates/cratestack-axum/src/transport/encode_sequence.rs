use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use cratestack_core::{CoolError, RouteTransportCapabilities};
use futures_util::{Stream, TryStreamExt};
use serde::Serialize;

use super::CBOR_SEQUENCE_CONTENT_TYPE;
use super::http_transport::HttpTransport;
use super::internal::fallback_error_response;
use super::media_type::select_transport_response_content_type;

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
    let content_type =
        match select_transport_response_content_type(transport, headers, capabilities) {
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

/// Genuinely incremental counterpart to
/// [`encode_transport_sequence_result_with_status_for`] for `@stream`
/// procedures (cratestack#283): `result` carries the still-unconsumed
/// item `Stream` rather than an already-collected `Vec`. `Err` here
/// means a *preflight* failure (authorization, before anything was
/// produced) — the ordinary buffered error path applies, since nothing
/// has streamed to the client yet. A failure *during* the stream is a
/// different thing entirely and never reaches this function as an
/// `Err`: it's absorbed into the item stream itself as the tag-48900
/// sentinel (see `super::stream_sequence`).
///
/// Only `application/cbor-seq` gets the truly incremental path. Any
/// other negotiated content type (plain JSON/CBOR array) falls back to
/// draining the stream into a `Vec` first and reusing the existing
/// buffered encoder — arrays can't be flushed incrementally the same
/// way, and `docs/design/rpc-transport.md` §3.3 only specifies
/// incremental delivery for cbor-seq. This keeps a `@stream` procedure
/// requested with a plain `Accept` behaving exactly like it did before
/// this ticket.
pub async fn encode_transport_stream_result_with_status_for<TTransport, TValue, S>(
    transport: &TTransport,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
    success_status: StatusCode,
    result: Result<S, CoolError>,
) -> Response
where
    TTransport: HttpTransport,
    TValue: Serialize + Send + 'static,
    S: Stream<Item = Result<TValue, CoolError>> + Send + 'static,
{
    if !capabilities.supports_sequence_response {
        return fallback_error_response(CoolError::Internal(
            "sequence response encoding requested for a route without sequence capability"
                .to_owned(),
        ));
    }
    let content_type =
        match select_transport_response_content_type(transport, headers, capabilities) {
            Ok(content_type) => content_type,
            Err(error) => return fallback_error_response(error),
        };
    match result {
        Ok(stream) => {
            if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
                transport
                    .encode_sequence_stream_response(content_type, success_status, stream)
                    .unwrap_or_else(fallback_error_response)
            } else {
                let values: Result<Vec<TValue>, CoolError> = stream.try_collect().await;
                encode_transport_sequence_result_with_status_for(
                    transport,
                    headers,
                    capabilities,
                    success_status,
                    values,
                )
            }
        }
        Err(error) => {
            let status = error.status_code();
            let body = error.into_response();
            transport
                .encode_sequence_error_response(content_type, status, &body)
                .unwrap_or_else(fallback_error_response)
        }
    }
}

#[cfg(test)]
mod tests;
