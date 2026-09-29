//! A sealed request is never marked replayable (cratestack#1007).
//!
//! Every sealed request carries a fresh `cti`, and the server answers a
//! replayed one with an unsigned `401`, so a retry layer that replays the
//! bytes would turn a transient `503` into a hard failure. The client marks
//! every sealed request `RequestIdempotency::new(false)`, whatever its
//! method, so an idempotency-aware middleware leaves it alone.
//!
//! `#![cfg(...)]` rather than `required-features`, like `middleware.rs`: it
//! compiles to an empty binary unless both features are on.

#![cfg(all(feature = "cose", feature = "middleware"))]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use cratestack_client_rust::cose::{
    CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver,
};
use cratestack_client_rust::{
    CborCodec, ClientConfig, ClientEnvelope, ClientError, CratestackClient, EnvelopeError,
    RequestIdempotency, RouteRef, ensure_crypto_provider,
};
use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{ClientBuilder, Middleware, Next};
use url::Url;

static SCHEMA_SHA: [u8; 32] = [7; 32];

/// Retries a `503`, but only a request marked idempotent.
struct RetryIfIdempotent {
    attempts: Arc<AtomicUsize>,
    seen: Arc<std::sync::Mutex<Vec<RequestIdempotency>>>,
}

#[async_trait::async_trait]
impl Middleware for RetryIfIdempotent {
    async fn handle(
        &self,
        request: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let marked = extensions
            .get::<RequestIdempotency>()
            .copied()
            .unwrap_or(RequestIdempotency::NOT_IDEMPOTENT);
        self.seen.lock().unwrap().push(marked);
        for _ in 0..3 {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let attempt = request.try_clone().expect("buffered body");
            let response = next.clone().run(attempt, extensions).await?;
            if !marked.is_idempotent() || response.status() != StatusCode::SERVICE_UNAVAILABLE {
                return Ok(response);
            }
        }
        unreachable!("the loop returns on its last pass")
    }
}

#[tokio::test]
async fn a_sealed_get_is_not_replayed_by_a_retry_layer() {
    ensure_crypto_provider();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().route(
        "/widgets",
        get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let http = ClientBuilder::new(reqwest::Client::new())
        .with(RetryIfIdempotent {
            attempts: attempts.clone(),
            seen: seen.clone(),
        })
        .build();
    let signer = HmacSigner::new(CoseAlg::Hmac256_64, vec![9; 32]).unwrap();
    let envelope = CoseEnvelope::client(
        CoseMode::Mac0,
        Arc::new(signer.clone()),
        Arc::new(StaticVerifierResolver::new().with_key(signer.verify_key())),
    )
    .build()
    .unwrap();
    let client = CratestackClient::with_middleware_client(
        ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap()),
        CborCodec,
        http,
    )
    .with_envelope(ClientEnvelope::new(envelope, "payments").unwrap())
    .unwrap()
    .with_schema_sha_bytes(&SCHEMA_SHA);

    let error = client
        .at(RouteRef::new("/widgets", &[]))
        .get::<serde_json::Value>("/widgets", &[], &[])
        .await
        .expect_err("the server answered 503, unsigned");
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 503 })
        ),
        "{error:?}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a sealed GET is sent once"
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [RequestIdempotency::NOT_IDEMPOTENT],
        "a GET is idempotent by method, but sealed it is not"
    );
}
