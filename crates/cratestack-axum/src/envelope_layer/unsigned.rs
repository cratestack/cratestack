//! A plain request under `Optional` (decision D10): it runs unsigned, and
//! its response is sealed only when it carries a valid `Cratestack-Nonce`
//! and the [`super::ResponseSealPolicy`] asks for it.

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest_unsigned};
use http::request::Parts;

use super::mode::EnvelopeMode;
use super::seal::{BindingInputs, Sealer};
use super::seal_policy::UnsignedRequest;
use super::service::Inner;
use super::{media, refusal, request};

pub(super) async fn handle<S: Inner>(
    inner: S,
    mut parts: Parts,
    body: Body,
    inputs: BindingInputs,
) -> Response {
    let config = inputs.config.clone();
    let Some(nonce) = nonce(&parts) else {
        return super::service::call(inner, Request::from_parts(parts, body)).await;
    };
    let view = UnsignedRequest {
        method: &inputs.method,
        route: &inputs.route,
        headers: &parts.headers,
        accept_names_envelope: media::accept_names_envelope(&parts.headers, &*config.envelope),
    };
    if !config.seal_policy.seal_unsigned(&view) {
        return super::service::call(inner, Request::from_parts(parts, body)).await;
    }

    let path = parts.uri.path().to_owned();
    let Some(payload) = request::buffer(body, config.max_body_bytes).await else {
        return refusal::too_large(&parts.headers, &path);
    };
    let digest = request_digest_unsigned(&nonce, &payload);
    request::rewrite_accept(&mut parts.headers, EnvelopeMode::Optional);
    let sealer = Sealer {
        inputs,
        request: digest,
        mode: EnvelopeMode::Optional,
        headers: parts.headers.clone(),
        path,
    };
    let response =
        super::service::call(inner, Request::from_parts(parts, Body::from(payload))).await;
    sealer.finish(response).await
}

/// Exactly one well-formed `Cratestack-Nonce`. Two headers are treated as
/// none: which one the client hashed is not ours to guess.
fn nonce(parts: &Parts) -> Option<RequestNonce> {
    let mut values = parts.headers.get_all(NONCE_HEADER).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return None;
    };
    RequestNonce::from_header_value(value.as_bytes()).ok()
}
