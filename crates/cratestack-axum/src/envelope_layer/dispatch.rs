//! Per request: which of the paths (signed, unsigned under `Optional`,
//! untouched, refused) a request takes.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use http::Method;
use http::request::Parts;

use super::layer::Config;
use super::mode::EnvelopeMode;
use super::policy_request::PolicyRequest;
use super::resolver::{Resolution, ResolvedRoute, RouteRequest};
use super::service::{Inner, call};
use super::{batch, media, refusal, request, signed, unresolved, unsigned};

pub(super) async fn dispatch<S: Inner>(config: Arc<Config>, inner: S, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();
    // Decided by the layer, not by any plug-in: a COSE body is opened or
    // refused, never forwarded.
    let enveloped = media::is_envelope_request(&parts.headers, &*config.envelope);
    // A CORS preflight carries no body and cannot be signed; it passes on
    // REST and RPC alike, whatever the policy (security-review nit).
    if parts.method == Method::OPTIONS && !enveloped && request::is_bodiless(&body) {
        return call(inner, Request::from_parts(parts, body)).await;
    }
    let (matched, params) = request::matched_route(&mut parts).await;
    // Once per request: both bindings are built from this one answer.
    let resolution = config.resolver.resolve(&RouteRequest::new(
        &parts.method,
        parts.uri.path(),
        matched.as_deref(),
        &params,
    ));
    let route = match resolution {
        Resolution::Op(route) => route,
        other => {
            let request = Request::from_parts(parts, body);
            return unresolved::handle(&config, inner, request, enveloped, matched, other).await;
        }
    };
    if route.is_batch() {
        return batch::handle(config, inner, parts, body, route, enveloped).await;
    }
    let mode = config.policy.mode(&policy_request(&parts.method, &route));
    by_mode(config, inner, parts, body, route, enveloped, mode).await
}

/// What the policy is asked about `route` (decision B1: a subscription's
/// bare op id, flagged).
fn policy_request<'a>(method: &'a Method, route: &'a ResolvedRoute) -> PolicyRequest<'a> {
    let request = PolicyRequest::new(method, route.op());
    if route.is_subscription() {
        request.subscription()
    } else {
        request
    }
}

pub(super) async fn by_mode<S: Inner>(
    config: Arc<Config>,
    inner: S,
    parts: Parts,
    body: Body,
    route: ResolvedRoute,
    enveloped: bool,
    mode: EnvelopeMode,
) -> Response {
    match (enveloped, mode) {
        (true, EnvelopeMode::Off) => {
            refusal::unsupported_envelope(&parts.headers, parts.uri.path())
        }
        (true, EnvelopeMode::Required | EnvelopeMode::Optional) => {
            signed::handle(config, inner, parts, body, route).await
        }
        (false, EnvelopeMode::Required) => {
            refusal::unauthenticated(&parts.headers, parts.uri.path())
        }
        (false, EnvelopeMode::Optional) => {
            unsigned::handle(config, inner, parts, body, route).await
        }
        (false, EnvelopeMode::Off) => call(inner, Request::from_parts(parts, body)).await,
    }
}
