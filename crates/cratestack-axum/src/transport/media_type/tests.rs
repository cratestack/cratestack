//! Unit coverage for cratestack#489: `select_transport_response_content_type`
//! must only ever return a `Content-Type` the concrete transport can
//! actually encode, never just one named in the route's static
//! `response_types` list.

use axum::http::{HeaderMap, HeaderValue, header};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::RouteTransportCapabilities;

use super::*;
use crate::codec::CodecSet;

/// Mirrors what the macro layer actually emits for every model/procedure
/// route (`procedure_transport_capabilities_tokens`,
/// `model_write_transport_capabilities_tokens`,
/// `RPC_BINDING_CAPABILITIES`): both codecs advertised, CBOR default —
/// regardless of which codec(s) the router was actually built with.
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

#[test]
fn json_only_router_picks_json_even_when_accept_also_lists_cbor() {
    let headers = headers_with_accept("application/json, application/cbor");
    let content_type =
        select_transport_response_content_type(&JsonCodec, &headers, &write_capabilities())
            .expect("json is genuinely encodable, must not 406");
    assert_eq!(content_type, "application/json");
}

#[test]
fn json_only_router_rejects_an_accept_naming_only_cbor() {
    let headers = headers_with_accept("application/cbor");
    let error = select_transport_response_content_type(&JsonCodec, &headers, &write_capabilities())
        .expect_err("router has no CBOR encoder, must 406");
    assert!(matches!(
        error,
        cratestack_core::CoolError::NotAcceptable(_)
    ));
    let message = error.public_message();
    assert!(
        message.contains("application/json"),
        "message should name what the router actually serves: {message}"
    );
    assert!(
        !message.contains("application/cbor"),
        "message must not claim an encoder that doesn't exist: {message}"
    );
}

#[test]
fn json_only_router_with_no_accept_header_falls_back_to_json_not_the_static_default() {
    // No explicit Accept at all — the static default is "application/cbor"
    // (baked in for every route regardless of which codec the router was
    // actually built with), which a JsonCodec-only router can't produce.
    let headers = HeaderMap::new();
    let content_type =
        select_transport_response_content_type(&JsonCodec, &headers, &write_capabilities())
            .expect("must fall back to something the router can actually encode");
    assert_eq!(content_type, "application/json");
}

#[test]
fn cbor_only_router_still_uses_the_static_default_when_it_can_encode_it() {
    let headers = HeaderMap::new();
    let content_type =
        select_transport_response_content_type(&CborCodec, &headers, &write_capabilities())
            .expect("cbor default is genuinely encodable here");
    assert_eq!(content_type, "application/cbor");
}

#[test]
fn codec_set_negotiates_either_codec() {
    let codec = CodecSet::new(CborCodec, JsonCodec);

    let cbor_headers = headers_with_accept("application/cbor");
    assert_eq!(
        select_transport_response_content_type(&codec, &cbor_headers, &write_capabilities())
            .unwrap(),
        "application/cbor"
    );

    let json_headers = headers_with_accept("application/json");
    assert_eq!(
        select_transport_response_content_type(&codec, &json_headers, &write_capabilities())
            .unwrap(),
        "application/json"
    );
}

#[test]
fn unsatisfiable_accept_against_a_two_codec_router_still_406s() {
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let headers = headers_with_accept("text/plain");
    let error = select_transport_response_content_type(&codec, &headers, &write_capabilities())
        .expect_err("neither codec speaks text/plain");
    assert!(matches!(
        error,
        cratestack_core::CoolError::NotAcceptable(_)
    ));
}

// -----------------------------------------------------------------------------
// `select_response_content_type` — the negotiation function itself, tested
// directly against an `encodable` list rather than through a transport, so
// these don't depend on any particular codec. Reproduces the original bug:
// this used to walk `encodable` (the *server's* order) and return the first
// entry the client merely tolerated, ignoring `Accept`'s own ordering and
// `q=` weights entirely — RFC 9110 §12.5.1 requires the opposite. That
// silently broke `rpc-streaming-client-rust`, whose client sends `Accept:
// application/cbor-seq, application/cbor` to prefer streaming and degrade
// gracefully, but always got buffered `application/cbor` back because
// `encodable`'s server-declared order (`rest.rs`'s
// `procedure_transport_capabilities_tokens`) puts plain cbor first.
// -----------------------------------------------------------------------------

/// Same three-way `response_types` shape a `T[]` procedure/model route
/// advertises (`procedure_transport_capabilities_tokens`'s list branch):
/// cbor, json, cbor-seq — in that server-declared order, deliberately
/// *not* client-preference order, so a test that passes by accident of
/// list order would be exposed by reordering it.
const SEQUENCE_ENCODABLE: &[&str] = &[
    "application/cbor",
    "application/json",
    "application/cbor-seq",
];
const SEQUENCE_DEFAULT: &str = "application/cbor";

#[test]
fn client_preferring_cbor_seq_gets_cbor_seq_even_though_encodable_lists_cbor_first() {
    // The generated Rust client's real Accept header for a streaming call
    // (`HttpClientCodec::sequence_accept_header_value`).
    let headers = headers_with_accept("application/cbor-seq, application/cbor");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(
        picked, "application/cbor-seq",
        "client's Accept order must win over encodable's server-declared order"
    );
}

#[test]
fn client_preferring_cbor_gets_cbor_even_though_cbor_seq_is_also_offered() {
    let headers = headers_with_accept("application/cbor, application/cbor-seq");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(picked, "application/cbor");
}

#[test]
fn q_values_override_both_the_accept_headers_own_order_and_encodables_order() {
    // `application/cbor` is listed FIRST in the Accept header AND first
    // in `SEQUENCE_ENCODABLE` (server order) — but weighted lower than
    // cbor-seq. The higher `q` must win over both tie-break signals, so
    // this only passes under real q-aware negotiation: a version that
    // merely honors Accept order (ignoring `q=`) would still wrongly
    // pick cbor here, same as the pre-fix "encodable order" bug did.
    let headers = headers_with_accept("application/cbor;q=0.5, application/cbor-seq;q=0.9");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(picked, "application/cbor-seq");
}

#[test]
fn q_zero_excludes_a_type_entirely_per_rfc_9110() {
    let headers = headers_with_accept("application/cbor;q=0, application/cbor-seq");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(
        picked, "application/cbor-seq",
        "q=0 must be treated as an explicit rejection, not just a low preference"
    );
}

#[test]
fn type_wildcard_matches_any_application_subtype_and_still_respects_q() {
    let headers = headers_with_accept("application/*;q=0.3, application/json;q=0.9");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(
        picked, "application/json",
        "an exact match's higher q must beat a wildcard match's lower q"
    );
}

#[test]
fn bare_wildcard_with_no_other_signal_falls_back_to_encodables_own_order() {
    // `*/*` alone gives every candidate an identical rank — the only
    // remaining tie-break is `encodable`'s (server-declared) order, so a
    // bare wildcard behaves exactly like "no preference expressed."
    let headers = headers_with_accept("*/*");
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(picked, SEQUENCE_ENCODABLE[0]);
}

#[test]
fn client_accepting_neither_offered_type_gets_406() {
    let headers = headers_with_accept("text/plain, image/png");
    let error = select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT)
        .expect_err("router offers only application/* types, must 406");
    assert!(matches!(
        error,
        cratestack_core::CoolError::NotAcceptable(_)
    ));
}

#[test]
fn no_accept_header_still_falls_back_to_default_unchanged_by_this_fix() {
    let headers = HeaderMap::new();
    let picked =
        select_response_content_type(&headers, SEQUENCE_ENCODABLE, SEQUENCE_DEFAULT).unwrap();
    assert_eq!(picked, SEQUENCE_DEFAULT);
}
