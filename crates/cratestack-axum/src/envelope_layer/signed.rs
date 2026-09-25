//! A request with an envelope body: open it, hand the router the payload,
//! seal whatever comes back.

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_cose::request_digest;
use http::request::Parts;

use super::mode::EnvelopeMode;
use super::principal::VerifiedRequest;
use super::seal::{BindingInputs, Sealer};
use super::service::Inner;
use super::{refusal, request};
use crate::ratelimit::VerifiedPrincipal;

pub(super) async fn handle<S: Inner>(
    inner: S,
    mut parts: Parts,
    body: Body,
    inputs: BindingInputs,
    mode: EnvelopeMode,
) -> Response {
    let path = parts.uri.path().to_owned();
    let config = inputs.config.clone();
    let Some(raw) = request::buffer(body, config.max_body_bytes).await else {
        return refusal::too_large(&parts.headers, &path);
    };

    let params = inputs.params();
    let bind = inputs.binding(&params, None);
    let opened = match config.envelope.open_request(raw.clone(), &bind).await {
        Ok(opened) => opened,
        // Whatever the envelope said, the peer learns only "401" (§10).
        Err(CratestackError::Unauthorized(_)) => {
            return refusal::unauthenticated(&parts.headers, &path);
        }
        Err(error) => return refusal::internal(&parts.headers, &path, "open", &error),
    };
    drop(params);
    // Over the bytes exactly as received, never a re-encoding: this is
    // what the client hashes too (ADR 0006 §4).
    let digest = request_digest(&raw);
    drop(raw);

    let (payload, signer) = opened.into_parts();
    let principal = config.principal.principal(&VerifiedRequest::new(
        &signer,
        &inputs.method,
        &inputs.route,
    ));

    request::rewrite_opened(&mut parts.headers, &payload);
    request::rewrite_accept(&mut parts.headers, mode);
    let sealer = Sealer {
        inputs,
        request: digest,
        mode,
        headers: parts.headers.clone(),
        path,
    };
    if principal.is_empty() {
        let error = CratestackError::Internal("the principal mapper returned \"\"".to_owned());
        return sealer.seal_error(error.status_code(), error).await;
    }
    parts.extensions.insert(signer);
    parts.extensions.insert(VerifiedPrincipal(principal));

    let response =
        super::service::call(inner, Request::from_parts(parts, Body::from(payload))).await;
    sealer.finish(response).await
}
