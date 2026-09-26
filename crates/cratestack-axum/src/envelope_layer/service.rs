//! [`EnvelopeService`]: the tower service the layer produces. The
//! per-request decisions are in `dispatch`.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::Response;
use tower::{Service, ServiceExt};

use super::dispatch::dispatch;
use super::layer::Config;

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

pub(super) async fn call<S: Inner>(inner: S, req: Request) -> Response {
    match inner.oneshot(req).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}
