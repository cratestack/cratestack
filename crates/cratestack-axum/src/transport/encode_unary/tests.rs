//! cratestack#489 acceptance coverage: `encode_transport_result_with_status_for`
//! is the exact function every generated model/procedure handler calls to
//! produce its response, with the exact capability shape codegen emits
//! (`cratestack-macros/src/transport/rest.rs`'s
//! `model_write_transport_capabilities_tokens`/
//! `procedure_transport_capabilities_tokens`: both `application/cbor` and
//! `application/json` advertised, `application/cbor` default — regardless
//! of which codec(s) the router was actually built with). These tests
//! reproduce the reported symptom (`406 Not Acceptable` /
//! `no encoder configured for response Content-Type application/cbor`,
//! cratestack/cratestack#489) directly against that function, with no
//! router/DB scaffolding needed since this is the function that owns the
//! whole content-negotiation decision.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CoolError, RouteTransportCapabilities};
use serde::Serialize;

use super::*;

#[derive(Serialize)]
struct Widget {
    name: &'static str,
}

/// Same shape `model_write_transport_capabilities_tokens`/
/// `procedure_transport_capabilities_tokens` generate for every route,
/// independent of which codec(s) the router actually registers.
fn capabilities() -> RouteTransportCapabilities {
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

/// The exact repro from the issue: a router built with `JsonCodec` alone
/// (`router(db, procedures, JsonCodec, auth, body_limit_bytes)`), a client honestly
/// advertising `Accept: application/json, application/cbor` (both
/// `cratestack_client_rust::JsonCodec`'s real default and every browser's
/// `fetch` default of appending `*/*`-like breadth). On `main` this
/// returns 406 with `no encoder configured for response Content-Type
/// application/cbor`, because `select_response_content_type` picked
/// `application/cbor` — the first entry of the *static* capability list
/// that also appears in `Accept` — without ever checking whether the
/// `JsonCodec` router passed in can actually produce it.
#[test]
fn json_only_router_serves_200_json_even_when_accept_also_lists_cbor() {
    let headers = headers_with_accept("application/json, application/cbor");
    let response = encode_transport_result_with_status_for(
        &JsonCodec,
        &headers,
        &capabilities(),
        StatusCode::OK,
        Ok::<_, CoolError>(Widget { name: "gizmo" }),
    );

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "must not 406 for a Content-Type the router can genuinely produce"
    );
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
}

/// A genuinely unsatisfiable `Accept` — the router really has no CBOR
/// encoder — must still 406, with a message naming what the router
/// actually serves rather than the static (and here misleading)
/// capability list.
#[test]
fn json_only_router_still_406s_for_a_genuinely_unsatisfiable_accept() {
    let headers = headers_with_accept("application/cbor");
    let response = encode_transport_result_with_status_for(
        &JsonCodec,
        &headers,
        &capabilities(),
        StatusCode::OK,
        Ok::<_, CoolError>(Widget { name: "gizmo" }),
    );

    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}
