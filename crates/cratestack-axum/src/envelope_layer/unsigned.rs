//! A plain request under `Optional` (decision D10): it runs unsigned, and
//! its response is sealed only when it carries a valid `Cratestack-Nonce`
//! and the [`super::ResponseSealPolicy`] asks for it.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{NONCE_HEADER, RequestNonce, request_digest_unsigned};
use http::request::Parts;

use super::bound::Bound;
use super::layer::Config;
use super::opened::SealContext;
use super::resolver::ResolvedRoute;
use super::seal::{BindingInputs, Sealer};
use super::seal_policy::UnsignedRequest;
use super::service::{Inner, call};
use super::{media, refusal, request};

pub(super) async fn handle<S: Inner>(
    config: Arc<Config>,
    inner: S,
    mut parts: Parts,
    body: Body,
    route: ResolvedRoute,
) -> Response {
    let Some(nonce) = nonce(&parts) else {
        return call(inner, Request::from_parts(parts, body)).await;
    };
    let accept_names_envelope = media::accept_names_envelope(&parts.headers, &*config.envelope);
    let view = UnsignedRequest::new(&parts.method, &route, &parts.headers, accept_names_envelope);
    if !config.seal_policy.seal_unsigned(&view) {
        return call(inner, Request::from_parts(parts, body)).await;
    }

    let path = parts.uri.path().to_owned();
    let bound = match Bound::read(&parts.headers) {
        Ok(bound) => bound,
        Err(error) => return refusal::bad_request(&parts.headers, &path, error),
    };
    let payload = match request::buffer(body, config.max_body_bytes).await {
        Ok(payload) => payload,
        Err(error) => return refusal::unbuffered(&parts.headers, &path, error),
    };
    // Binds this nonce and this payload, and nothing about who sent them:
    // the request was not signed (see `ResponseSealPolicy`).
    let digest = request_digest_unsigned(&nonce, &payload);
    request::rewrite_accept(&mut parts.headers, false);
    let inputs = BindingInputs::new(config, parts.method.clone(), route, &parts.uri, bound);
    let sealer = Sealer {
        inputs,
        request: digest,
        context: SealContext::empty(),
        strict: false,
        headers: parts.headers.clone(),
        path,
    };
    let response = call(inner, Request::from_parts(parts, Body::from(payload))).await;
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
