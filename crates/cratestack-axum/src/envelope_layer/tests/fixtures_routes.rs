//! The route table the hand-built REST routers are resolved against: the
//! descriptors a generated schema emits, one per route, with the payload
//! types each declares.

// Shared with the COSE suites, which the `envelope` feature alone does
// not compile; what only they use is dead there.
#![cfg_attr(not(feature = "cose"), allow(dead_code))]

use cratestack_core::{RouteTransportCapabilities, RouteTransportDescriptor};

const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &["application/cbor"],
    response_types: &["application/cbor"],
    default_response_type: "application/cbor",
    supports_sequence_response: false,
};

/// A route that takes and gives CBOR or JSON, and takes a form (the
/// generated routes declare CBOR and JSON; the form is vpay's).
const CAPS_ANY: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &[
        "application/cbor",
        "application/json",
        "application/x-www-form-urlencoded",
    ],
    response_types: &["application/cbor", "application/json"],
    default_response_type: "application/json",
    supports_sequence_response: false,
};

/// vpay's shape: a form in, JSON out, and nothing else.
const CAPS_FORM_JSON: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &["application/x-www-form-urlencoded"],
    response_types: &["application/json"],
    default_response_type: "application/json",
    supports_sequence_response: false,
};

/// What the generated model reads declare: `list_get`, `detail_get` and
/// `detail_delete` take no payload, so their `request_types` is empty, and
/// everywhere else an empty list means "no constraint". (A fixture that
/// gave a GET a request type hid the 415 a CBOR client got from them.)
const CAPS_READ: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &[],
    response_types: &["application/cbor", "application/json"],
    default_response_type: "application/cbor",
    supports_sequence_response: false,
};

const fn route(method: &'static str, path: &'static str) -> RouteTransportDescriptor {
    route_with(method, path, CAPS)
}

const fn route_with(
    method: &'static str,
    path: &'static str,
    capabilities: RouteTransportCapabilities,
) -> RouteTransportDescriptor {
    RouteTransportDescriptor {
        name: path,
        method,
        path,
        capabilities,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    }
}

pub static REST_ROUTES: [RouteTransportDescriptor; 12] = [
    route("POST", "/widgets"),
    route_with("GET", "/widgets/{id}", CAPS_READ),
    route_with("DELETE", "/widgets/{id}", CAPS_READ),
    route_with("GET", "/notes", CAPS_READ),
    route_with("GET", "/form-read", CAPS_FORM_JSON),
    route("GET", "/text-error"),
    route("GET", "/json"),
    route_with("GET", "/stream", CAPS_ANY),
    route_with("POST", "/pay", CAPS_FORM_JSON),
    route_with("POST", "/either", CAPS_ANY),
    route_with("GET", "/html", CAPS_ANY),
    route_with("GET", "/plain-error", CAPS_ANY),
];
