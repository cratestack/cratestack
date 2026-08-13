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
