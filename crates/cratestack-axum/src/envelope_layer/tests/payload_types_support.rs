//! What the payload-type suites share (cratestack#1168): the types, a layer
//! that opted in, and a request carrying the selector headers.

use axum::extract::Request;
use bytes::Bytes;
use http::Method;

use super::cose_support::cose_request;
use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

pub(super) const FORM: &str = "application/x-www-form-urlencoded";
pub(super) const JSON: &str = "application/json";
pub(super) const CBOR: &str = "application/cbor";

/// A layer that opted in to forms on the way in and JSON on the way out.
pub(super) fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .payload_media_types([CBOR, FORM, JSON], [CBOR, JSON])
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

pub(super) fn json_error_code(body: &[u8]) -> String {
    let value: serde_json::Value = serde_json::from_slice(body).expect("a JSON error body");
    value["code"].as_str().expect("code").to_owned()
}

/// `cose_request` plus the payload-type selector headers (cratestack#1168).
pub(super) fn typed_request(
    method: Method,
    uri: &str,
    body: Bytes,
    payload_type: Option<&str>,
    payload_accept: Option<&str>,
) -> Request {
    let mut req = cose_request(method, uri, body);
    if let Some(value) = payload_type {
        req.headers_mut().insert(
            cratestack_core::PAYLOAD_TYPE_HEADER,
            http::HeaderValue::from_str(value).expect("header"),
        );
    }
    if let Some(value) = payload_accept {
        req.headers_mut().insert(
            cratestack_core::PAYLOAD_ACCEPT_HEADER,
            http::HeaderValue::from_str(value).expect("header"),
        );
    }
    req
}
