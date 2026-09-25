//! [`EnvelopeService`]: the per-request dispatch between the three paths
//! (signed, unsigned under `Optional`, untouched) and the layer's refusals.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Once};
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::Response;
use tower::{Service, ServiceExt};

use super::layer::Config;
use super::mode::EnvelopeMode;
use super::resolver::RouteRequest;
use super::seal::BindingInputs;
use super::{media, refusal, request, signed, unsigned};

/// What the layer wraps: a route of the generated router.
pub(super) trait Inner:
    Service<Request, Response = Response, Error = Infallible, Future: Send> + Clone + Send + 'static
{
}

impl<S> Inner for S where
    S: Service<Request, Response = Response, Error = Infallible, Future: Send>
        + Clone
        + Send
        + 'static
{
}

/// The service [`super::EnvelopeLayer`] produces.
#[derive(Clone)]
pub struct EnvelopeService<S> {
    pub(super) inner: S,
    pub(super) config: Arc<Config>,
}

impl<S> Service<Request> for EnvelopeService<S>
where
    S: Service<Request, Response = Response, Error = Infallible, Future: Send>
        + Clone
        + Send
        + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    /// Always ready: each call drives a clone of the inner service through
    /// `oneshot`, which waits for that clone's readiness itself.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let inner = self.inner.clone();
        let config = self.config.clone();
        Box::pin(async move { Ok(dispatch(config, inner, req).await) })
    }
}

async fn dispatch<S: Inner>(config: Arc<Config>, inner: S, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();
    let (matched, params) = request::matched_route(&mut parts).await;
    // Once per request: both bindings are built from this one answer.
    let resolved = config.resolver.resolve(&RouteRequest {
        method: &parts.method,
        path: parts.uri.path(),
        matched_path: matched.as_deref(),
        path_params: &params,
    });
    // Decided by the layer, not by the policy: a COSE body is opened or
    // refused, never forwarded.
    let enveloped = media::is_envelope_request(&parts.headers, &*config.envelope);
    let Some(route) = resolved else {
        if enveloped {
            return refusal::unsupported_envelope(&parts.headers, parts.uri.path());
        }
        // Not a generated op (D6): an unmatched path, or a route the schema
        // did not generate. Untouched.
        if matched.is_some() {
            warn_unresolved_once();
        }
        return call(inner, Request::from_parts(parts, body)).await;
    };
    let mode = config.policy.mode(&parts.method, &route);
    let inputs = BindingInputs::new(config, parts.method.clone(), route, &parts.uri);
    match (enveloped, mode) {
        (true, EnvelopeMode::Off) => {
            refusal::unsupported_envelope(&parts.headers, parts.uri.path())
        }
        (true, EnvelopeMode::Required | EnvelopeMode::Optional) => {
            signed::handle(inner, parts, body, inputs, mode).await
        }
        (false, EnvelopeMode::Required) => {
            refusal::unauthenticated(&parts.headers, parts.uri.path())
        }
        (false, EnvelopeMode::Optional) => unsigned::handle(inner, parts, body, inputs).await,
        (false, EnvelopeMode::Off) => call(inner, Request::from_parts(parts, body)).await,
    }
}

pub(super) async fn call<S: Inner>(inner: S, req: Request) -> Response {
    match inner.oneshot(req).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// A route axum matched that the resolver does not know. Legitimate for a
/// hand-written route merged into the generated router before the layer,
/// but it is also exactly what a missing or wrong mount prefix looks like,
/// and then plain traffic to every op passes unsigned (D6). Logged once per
/// process, like the idempotency layer's missing-identity warning.
fn warn_unresolved_once() {
    static WARNING: Once = Once::new();
    WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "envelope",
            "the envelope layer saw a matched route its binding resolver does not know, and \
             let it through unsigned. If the generated router is nested, give the layer the \
             same prefix (rest(\"/api\", ..) / rpc(\"/api\")). Logged once per process.",
        );
    });
}
