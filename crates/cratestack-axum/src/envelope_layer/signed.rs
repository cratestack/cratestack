//! A request with an envelope body: open it, hand the router the payload,
//! seal whatever comes back. A signed request's response is always sealed,
//! under `Optional` as under `Required` (decision S3).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{CratestackError, request_digest};
use http::StatusCode;
use http::request::Parts;

use super::bound::Bound;
use super::layer::Config;
use super::mode::EnvelopeMode;
use super::principal::VerifiedRequest;
use super::resolver::ResolvedRoute;
use super::seal::{BindingInputs, Sealer, stream_refused};
use super::service::{Inner, call};
use super::{batch, refusal, request};
use crate::ratelimit::VerifiedPrincipal;

pub(super) async fn handle<S: Inner>(
    config: Arc<Config>,
    inner: S,
    mut parts: Parts,
    body: Body,
    route: ResolvedRoute,
) -> Response {
    let path = parts.uri.path().to_owned();
    let bound = match Bound::read(&parts.headers) {
        Ok(bound) => bound,
        Err(error) => return refusal::bad_request(&parts.headers, &path, error),
    };
    let raw = match request::buffer(body, config.max_body_bytes).await {
        Ok(raw) => raw,
        Err(error) => return refusal::unbuffered(&parts.headers, &path, error),
    };
    let inputs = BindingInputs::new(
        config.clone(),
        parts.method.clone(),
        route,
        &parts.uri,
        bound,
    );
    let opened = {
        let params = inputs.params();
        let bind = inputs.binding(&params, None);
        config.envelope.open_request(raw.clone(), &bind).await
    };
    let opened = match opened {
        Ok(opened) => opened,
        // Whatever the envelope said, the peer learns only "401" (§10).
        Err(CratestackError::Unauthorized(_)) => {
            return refusal::unauthenticated(&parts.headers, &path);
        }
        Err(error) => return refusal::internal(&parts.headers, &path, "open", &error),
    };
    // Over the bytes exactly as received, never a re-encoding: this is
    // what the client hashes too (ADR 0006 §4).
    let digest = request_digest(&raw);
    drop(raw);

    let (payload, signer, context) = opened.into_parts();
    request::rewrite_opened(&mut parts.headers, &payload);
    request::rewrite_accept(&mut parts.headers, true);
    let sealer = Sealer {
        inputs,
        request: digest,
        context,
        strict: true,
        headers: parts.headers.clone(),
        path,
    };
    // A subscription streams, which cannot be sealed yet: refused before
    // its handler runs (API-review decision), not after.
    if sealer.inputs.route.is_subscription() {
        return sealer
            .seal_error(StatusCode::NOT_ACCEPTABLE, stream_refused())
            .await;
    }
    if sealer.inputs.route.is_batch() {
        match batch::signed_verdict(&config, &sealer.inputs.method, &payload) {
            Err(error) => return sealer.seal_error(StatusCode::BAD_REQUEST, error).await,
            Ok(EnvelopeMode::Off) => {
                return refusal::unsupported_envelope(&sealer.headers, &sealer.path);
            }
            Ok(_) => {}
        }
    }

    let verified = VerifiedRequest::new(&signer, &sealer.inputs.method, &sealer.inputs.route);
    let principal = match config.principal.principal(&verified).await {
        Ok(principal) if !principal.is_empty() => principal,
        Ok(_) => {
            let error = CratestackError::Internal("the principal mapper returned \"\"".to_owned());
            return sealer
                .seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                .await;
        }
        // A signer the mapper refuses is answered like one that did not
        // verify: the same unsigned, coarse 401.
        Err(CratestackError::Unauthorized(_)) => {
            return refusal::unauthenticated(&sealer.headers, &sealer.path);
        }
        Err(error) => {
            let error = CratestackError::Internal(format!("the principal mapper failed: {error}"));
            return sealer
                .seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                .await;
        }
    };
    parts.extensions.insert(signer);
    parts.extensions.insert(VerifiedPrincipal(principal));

    let response = call(inner, Request::from_parts(parts, Body::from(payload))).await;
    sealer.finish(response).await
}
