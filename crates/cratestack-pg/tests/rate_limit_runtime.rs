//! Runtime test for rate limiting: verify that `@no_rate_limit` procedures
//! are actually exempt from throttling, while un-annotated procedures are throttled.
//! (Fixes cratestack#474.)
//!
//! This test drives REAL HTTP requests against a live RPC server and asserts
//! that status codes match expectations — proving the exemption is selective
//! rather than a blanket disable.

#![cfg(all(feature = "rate_limit", feature = "codec-json"))]

use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack_axum::ratelimit::{
    InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer, build_rpc_ops_filter,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceBuilder;

include_server_schema!("tests/fixtures/rate_limit_extension.cstack", db = Postgres);

mod support;

use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext, Value};

#[allow(dead_code)]
fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct RpcProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for RpcProcedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn create_payment(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::create_payment::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::create_payment::Output, CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }
}

/// AC 1: Given `@no_rate_limit` on a procedure, when the rate limit is
/// exceeded, then the request is not throttled (succeeds with status 200).
///
/// AC 2: Given no annotation, when the rate limit is exceeded, then the
/// request is throttled (fails with status 429), proving the exemption is
/// selective.
#[tokio::test]
#[ignore = "requires database: set CRATESTACK_REQUIRE_DB=1 and DOCKER_HOST"]
async fn rate_limit_exemption_is_selective() {
    let db = test_db();
    let codec = cratestack_codec_json::JsonCodec;
    let auth = AlwaysAuthProvider;

    // Spin up an RPC server with a very tight rate limit (1 req/sec burst)
    // so we can exceed it quickly in a single test.
    let rate_limit_store = Arc::new(InMemoryRateLimitStore::default());
    let rate_limit_config = RateLimitConfig::new(1, 1.0); // 1-req burst, 1.0 req/sec refill
    let ops = cratestack_schema::axum::OPS;
    let rpc_filter = build_rpc_ops_filter(ops);

    let mut router: Router =
        cratestack_schema::axum::rpc_router(db.clone(), RpcProcedures, codec, auth);

    // Apply rate limiting with the operation filter.
    router = router.layer(
        ServiceBuilder::new()
            .layer(
                RateLimitLayer::new(rate_limit_store.clone(), rate_limit_config)
                    .with_should_rate_limit_fn(rpc_filter),
            )
            .into_inner(),
    );

    // Start a test server on a random port.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind to random port");
    let addr = listener.local_addr().expect("should get local address");
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server should run")
    });

    // Make requests and verify rate limiting behavior.
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // Ping: normal procedure with no exemption. Should be rate-limited.
    {
        let ping_url = format!("{}/rpc/procedure.ping", base_url);

        // First request should succeed (within the 1-req burst).
        let resp1 = client
            .post(&ping_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"test1"}"#)
            .send()
            .await
            .expect("first ping should send");
        assert_eq!(
            resp1.status(),
            StatusCode::OK,
            "first ping should succeed (within burst)"
        );

        // Second request immediately after should be throttled (429).
        let resp2 = client
            .post(&ping_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"test2"}"#)
            .send()
            .await
            .expect("second ping should send");
        assert_eq!(
            resp2.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "second ping should be throttled (rate limit exceeded)"
        );
    }

    // CreatePayment: procedure with `@no_rate_limit`. Should NOT be rate-limited.
    {
        let payment_url = format!("{}/rpc/procedure.createPayment", base_url);

        // First request should succeed.
        let resp1 = client
            .post(&payment_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"payment1"}"#)
            .send()
            .await
            .expect("first payment should send");
        assert_eq!(
            resp1.status(),
            StatusCode::OK,
            "first payment should succeed"
        );

        // Second request immediately after should ALSO succeed (not throttled).
        let resp2 = client
            .post(&payment_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"payment2"}"#)
            .send()
            .await
            .expect("second payment should send");
        assert_eq!(
            resp2.status(),
            StatusCode::OK,
            "second payment should succeed (exempted from rate limit)"
        );

        // Third request should still succeed, proving the exemption is not
        // a blanket disable of rate limiting — it's selective to this op.
        let resp3 = client
            .post(&payment_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"payment3"}"#)
            .send()
            .await
            .expect("third payment should send");
        assert_eq!(
            resp3.status(),
            StatusCode::OK,
            "third payment should also succeed (rate limit exempt)"
        );
    }

    // Verify that ping is still throttled (not a blanket disable).
    {
        let ping_url = format!("{}/rpc/procedure.ping", base_url);

        // Subsequent ping requests should still be throttled.
        let resp = client
            .post(&ping_url)
            .header("content-type", "application/json")
            .body(r#"{"nonce":"test3"}"#)
            .send()
            .await
            .expect("ping should send");
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "ping should remain throttled (not a blanket disable)"
        );
    }

    server_handle.abort();
}

#[test]
fn ops_filter_extracts_rpc_paths() {
    // Smoke test for the op extraction logic.
    // (This is unit-testable without a database.)

    let ops = cratestack_schema::axum::OPS;
    let filter = build_rpc_ops_filter(ops);

    // Mock a request to `/rpc/procedure.createPayment`.
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/rpc/procedure.createPayment")
        .body(Body::empty())
        .expect("should build request");

    // Should NOT rate-limit this op (it has @no_rate_limit).
    assert!(!filter(&req), "procedure.createPayment should be exempt");

    // Mock a request to `/rpc/procedure.ping` (no exemption).
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/rpc/procedure.ping")
        .body(Body::empty())
        .expect("should build request");

    // Should rate-limit this op.
    assert!(filter(&req), "procedure.ping should be rate-limited");

    // Mock a request to `/rpc/batch` (not an op, is a dispatch point).
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/rpc/batch")
        .body(Body::empty())
        .expect("should build request");

    // Framework dispatch points should be rate-limited.
    assert!(filter(&req), "/rpc/batch should be rate-limited");

    // Non-RPC path should rate-limit (fail closed).
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/widgets")
        .body(Body::empty())
        .expect("should build request");

    assert!(
        filter(&req),
        "non-RPC paths should be rate-limited by default"
    );
}
