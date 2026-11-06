//! [`CompactService`]: the per-request flow.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::compact::OpKeys;
use tower::Service;

use super::layer::CompactLayer;
use super::{negotiate, request, resolve, response};

/// The service [`CompactLayer`] wraps a router in.
#[derive(Clone)]
pub struct CompactService<S> {
    pub(super) inner: S,
    pub(super) layer: CompactLayer,
}

impl<S> Service<Request> for CompactService<S>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        // The clone is the not-yet-ready one; the ready service stays here.
        let ready = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, ready);
        let layer = self.layer.clone();
        Box::pin(async move {
            let wanted = negotiate::accepted_compact(req.headers());
            let sent = negotiate::request_compact(req.headers());
            if wanted.is_empty() && sent.is_none() {
                return inner.call(req).await;
            }
            let keys: Option<OpKeys> = negotiate::selector(req.headers())
                .and_then(|selector| resolve::resolve(&layer.tables, &req, &selector));
            let mut req = req;
            if let Some(compact) = sent {
                match request::open(&layer, keys.as_ref(), compact, req).await {
                    Ok(opened) => req = opened,
                    Err(refusal) => return Ok(*refusal),
                }
            }
            let (path, headers) = (req.uri().path().to_owned(), req.headers().clone());
            if !wanted.is_empty() {
                negotiate::rewrite_accept(req.headers_mut());
            }
            let answer = inner.call(req).await?;
            Ok(match keys {
                Some(keys) if !wanted.is_empty() => {
                    response::compact(&layer, &keys, &wanted, &headers, &path, answer).await
                }
                _ => answer,
            })
        })
    }
}
