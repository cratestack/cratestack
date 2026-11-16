//! A router standing in for a generated `transport rpc` one, with a
//! subscription handler that serves through `serve_subscription_with` as the
//! generated ones do, and reports what the layer handed it in `x-seen-*`.

use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::{get, post};
use bytes::Bytes;
use cratestack_core::{CratestackError, FramesMode, SubscriptionStream};
use futures_util::stream;
use http::{HeaderValue, Method, header};
use tokio::time::Instant;

use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};
use crate::rpc::{
    SignedStreamDeadline, encode_subscribe_error, negotiate_subscribe_binding,
    serve_subscription_with,
};

/// The marked subscription, and one the schema does not mark.
pub const MARKED: &str = "model.Widget.subscribe";
pub const UNMARKED: &str = "procedure.read";
pub const MARKED_PATH: &str = "/rpc/subscribe/model.Widget.subscribe";
pub const TABLE: &[(&str, FramesMode)] = &[(MARKED, FramesMode::Unsigned)];
pub const CBOR_SEQ: &str = "application/cbor-seq";
pub const SSE: &str = "text/event-stream";

pub fn builder() -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .subscriptions(TABLE)
}

/// What a subscribe handler does with an open that reached it.
#[derive(Clone, Copy)]
pub enum Serve {
    /// One item, then the stream ends.
    Items,
    /// Nothing, forever (until a deadline ends it).
    Quiet,
    /// A `403`, as `encode_subscribe_error` writes it.
    Forbidden,
}

async fn subscribe(hits: Hits, serve: Serve, req: Request) -> Response {
    hits.hit();
    let headers = req.headers().clone();
    let deadline = req.extensions().get::<SignedStreamDeadline>().copied();
    let principal = req
        .extensions()
        .get::<crate::ratelimit::VerifiedPrincipal>()
        .map_or("<absent>".to_owned(), |principal| principal.0.clone());
    let binding = match negotiate_subscribe_binding(&headers) {
        Ok(binding) => binding,
        Err(error) => return encode_subscribe_error(&error),
    };
    let items: SubscriptionStream<serde_json::Value> = match serve {
        Serve::Items => Box::pin(stream::iter([Ok(serde_json::json!({"n": 1}))])),
        Serve::Quiet => Box::pin(stream::pending()),
        Serve::Forbidden => {
            return encode_subscribe_error(&CratestackError::Forbidden("no".to_owned()));
        }
    };
    let mut response =
        serve_subscription_with(binding, req.extensions(), async { Ok(items) }).await;
    let seen = response.headers_mut();
    let accept = headers.get(header::ACCEPT).cloned();
    seen.insert(
        "x-seen-accept",
        accept.unwrap_or(HeaderValue::from_static("<absent>")),
    );
    seen.insert(
        "x-seen-principal",
        HeaderValue::from_str(&principal).expect("ascii"),
    );
    let left = deadline.map(|SignedStreamDeadline(at)| at - Instant::now());
    let left = left.map_or("<none>".to_owned(), |left| left.as_millis().to_string());
    seen.insert(
        "x-seen-deadline-ms",
        HeaderValue::from_str(&left).expect("ascii"),
    );
    response
}

/// `/rpc/subscribe/{op}` on GET and POST, and a unary `/rpc/{op}` that
/// answers a CBOR body.
pub fn router(layer: EnvelopeLayer, hits: &Hits, serve: Serve) -> Router {
    let (get_hits, post_hits) = (hits.clone(), hits.clone());
    Router::new()
        .route(
            "/rpc/subscribe/{op_id}",
            get(move |req: Request| subscribe(get_hits.clone(), serve, req))
                .post(move |req: Request| subscribe(post_hits.clone(), serve, req)),
        )
        .route(
            "/rpc/{op_id}",
            post(|| async { ([(header::CONTENT_TYPE, "application/cbor")], PAYLOAD) }),
        )
        .layer(layer)
}

/// A sealed open of `op` (`subscribe/<op>`) by `method`, asking for `accept`
/// (one header per entry), and the call to open its answer with.
pub async fn open(method: Method, op: &'static str, accept: &[&str]) -> (Request, Call, Bytes) {
    let path = format!("/rpc/subscribe/{op}");
    let route: &'static str = Box::leak(format!("subscribe/{op}").into_boxed_str());
    let call = Call::new(method.clone(), route, &[]);
    let payload: &[u8] = if method == Method::POST { PAYLOAD } else { &[] };
    let sealed = call.seal(payload).await;
    let mut request = cose_request(method, &path, sealed.clone());
    request.headers_mut().remove(header::ACCEPT);
    for value in accept {
        request
            .headers_mut()
            .append(header::ACCEPT, HeaderValue::from_str(value).expect("ascii"));
    }
    (request, call, sealed)
}

pub fn life(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}
