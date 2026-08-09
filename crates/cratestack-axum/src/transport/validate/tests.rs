//! cratestack#489: the `Accept` preflight
//! (`validate_transport_request_headers_for`/
//! `validate_transport_response_headers_for`) must reject a request that
//! only names content types the router has no encoder for *before* the
//! handler runs — a model `create`'s DB write, for one — not just leave
//! it to `encode_transport_result_with_status_for` to 406 only after the
//! side effect already happened.

use axum::http::{HeaderMap, HeaderValue, header};
use cratestack_codec_json::JsonCodec;
use cratestack_core::CoolError;

use super::*;

fn write_capabilities() -> RouteTransportCapabilities {
    RouteTransportCapabilities {
        request_types: &["application/cbor", "application/json"],
        response_types: &["application/cbor", "application/json"],
        default_response_type: "application/cbor",
        supports_sequence_response: false,
    }
}

fn headers_with_accept(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
    headers
}

/// `validate_transport_request_headers_for` also validates the request's
/// own `Content-Type` (unrelated to cratestack#489 — that check already
/// names something real, the request body's actual encoding) — give it
/// one so these tests isolate the `Accept`/response-encoder behavior
/// under test.
fn headers_with_accept_and_json_content_type(accept: &str) -> HeaderMap {
    let mut headers = headers_with_accept(accept);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers
}

#[test]
fn json_only_router_preflight_accepts_a_satisfiable_accept() {
    let headers = headers_with_accept_and_json_content_type("application/json, application/cbor");
    validate_transport_request_headers_for(&JsonCodec, &headers, &write_capabilities())
        .expect("json is genuinely encodable");
}

/// This is the DB-write-then-406 scenario from the issue's audit
/// request: an `Accept` naming only `application/cbor` against a
/// `JsonCodec`-only router must fail *here*, at preflight, before a
/// caller like `handle_create_dispatch` ever reaches
/// `state.db.model().create(...).run(&ctx)`.
#[test]
fn json_only_router_preflight_rejects_an_accept_naming_only_cbor() {
    let headers = headers_with_accept_and_json_content_type("application/cbor");
    let error = validate_transport_request_headers_for(&JsonCodec, &headers, &write_capabilities())
        .expect_err("router has no CBOR encoder — must fail before any handler side effect runs");
    assert!(matches!(error, CoolError::NotAcceptable(_)));
}

#[test]
fn json_only_router_preflight_allows_no_accept_header_at_all() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    validate_transport_request_headers_for(&JsonCodec, &headers, &write_capabilities())
        .expect("no Accept header means the caller hasn't constrained anything yet");
}

#[test]
fn response_headers_preflight_uses_the_same_codec_aware_check() {
    let headers = headers_with_accept("application/cbor");
    let error =
        validate_transport_response_headers_for(&JsonCodec, &headers, &write_capabilities())
            .expect_err("GET-style preflight must be equally honest about the router's encoders");
    assert!(matches!(error, CoolError::NotAcceptable(_)));
}
