//! A request with an envelope body: open it, hand the router the payload,
//! seal whatever comes back. A signed request's response is always sealed,
//! under `Optional` as under `Required` (decision S3), with one exception: a
//! signed open of a subscription marked `@frames(unsigned)` answers with an
//! unsealed stream (see `signed_subscription.rs`), in every mode that accepts
//! a signed request.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{CratestackError, request_digest};
use http::StatusCode;
use http::request::Parts;

use super::bound::Bound;
use super::contract::{self, Choice};
use super::layer::Config;
use super::mode::EnvelopeMode;
use super::resolver::ResolvedRoute;
use super::seal::{BindingInputs, Sealer, stream_refused};
use super::service::{Inner, call};
use super::signed_principal::admit;
use super::signed_subscription::{self, Frames};
use super::{batch, payload, refusal, request};

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
    // Before any key is looked up, any signature checked or any nonce spent.
    let negotiated = match payload::negotiate(&config, &route, &parts.method, &parts.headers, true)
    {
        Ok(negotiated) => negotiated,
        Err(error) => return refusal::payload_types(&parts.headers, &path, error),
    };
    let candidates = match contract::choose(&config, &parts.method, &route, &parts.headers) {
        Choice::Try(candidates) => candidates,
        Choice::Unsupported => return refusal::contract_unsupported(&parts.headers, &path),
        Choice::Malformed(error) => return refusal::bad_request(&parts.headers, &path, error),
        Choice::Misconfigured => return refusal::no_contract(&parts.headers, &path, &route),
    };
    let raw = match request::buffer(body, config.max_body_bytes).await {
        Ok(raw) => raw,
        Err(error) => return refusal::unbuffered(&parts.headers, &path, error),
    };
    let mut inputs = BindingInputs::new(
        config.clone(),
        parts.method.clone(),
        route,
        &parts.uri,
        bound,
        candidates[0],
        negotiated,
    );
    let opened = contract::open_under(&config, &raw, &mut inputs, &candidates).await;
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
    request::rewrite_opened(&mut parts.headers, &payload, &inputs.payload.request);
    // Read before `rewrite_accept` replaces it: a signed subscription open
    // names its stream type here.
    let accept = signed_subscription::single_accept(&parts.headers);
    request::rewrite_accept(&mut parts.headers, true, &inputs.payload.accept());
    let sealer = Sealer {
        inputs,
        request: digest,
        context,
        strict: true,
        headers: parts.headers.clone(),
        path,
    };
    // A read or delete names a type its empty payload need not have been
    // checked for; one that carries a payload is checked now, before the
    // handler, and answered sealed under the binding it was sent with.
    if payload::check_opened(
        &config,
        &sealer.inputs.route,
        &sealer.inputs.payload,
        &payload,
    )
    .is_err()
    {
        let error = CratestackError::UnsupportedMediaType(
            "the request payload's type is not accepted for this operation".to_owned(),
        );
        return sealer
            .seal_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, error)
            .await;
    }
    // A signed subscription open streams plain frames, and only a marked one
    // (`@frames(unsigned)`) may; any other is refused before its handler runs
    // (API-review decision), not after.
    match signed_subscription::lookup(&config, &sealer.inputs.method, &sealer.inputs.route) {
        Frames::Marked => {
            return signed_subscription::handle(
                &config, inner, parts, payload, signer, sealer, accept,
            )
            .await;
        }
        Frames::Unmarked => {
            return sealer
                .seal_error(StatusCode::NOT_ACCEPTABLE, stream_refused())
                .await;
        }
        Frames::NotSubscription => {}
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

    if let Some(response) = admit(&config, &sealer, signer, &mut parts.extensions).await {
        return response;
    }

    let response = call(inner, Request::from_parts(parts, Body::from(payload))).await;
    sealer.finish(response).await
}
