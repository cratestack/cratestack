//! cratestack#1039 review: the guard steps `rmcp` would echo a step later.
//!
//! A 405, a 413 and a refused second `Authorization` are all answers `rmcp`
//! (or the provider) would also give, so `http_guard.rs`'s status checks
//! pass with the guard's own step deleted. Each test here asserts what only
//! the guard's step guarantees: the application's provider never ran. The
//! `Host` and session tests pin two builder settings whose deletion no
//! other test notices.

mod support;

use axum::Router;
use axum::body::Body;
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::{Method, Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::counting::{Chunked, CountingProvider};
use support::http_app::{APP_ORIGIN, RESOURCE, mount, post, rpc, send, served, token, tool_call};
use support::token::{ISSUER, mint};

fn counted(tools: &FakeTools, resource: &str) -> (Router, CountingProvider) {
    let provider = CountingProvider::new(resource);
    let server = StreamableHttpServer::builder(
        tools.clone(),
        provider.clone(),
        [APP_ORIGIN],
        ProtectedResource::new(resource, [ISSUER]),
    )
    .build()
    .expect("valid configuration");
    (mount(&server), provider)
}

/// Without the guard's 405, a `GET` with a token reaches the provider and
/// then `rmcp` answers 405 itself, and one without a token gets a 401
/// challenge for an endpoint it can never call that way.
#[tokio::test]
async fn a_method_other_than_post_is_405_before_the_provider_runs() {
    let (app, provider) = counted(&FakeTools::default(), RESOURCE);
    let valid = token("u-1");
    for method in [Method::GET, Method::DELETE, Method::HEAD, Method::OPTIONS] {
        for bearer in [None, Some(valid.as_str())] {
            let mut request = Request::builder()
                .method(method.clone())
                .uri("/mcp")
                .header("host", "localhost")
                .header("accept", "application/json, text/event-stream");
            if let Some(bearer) = bearer {
                request = request.header("authorization", format!("Bearer {bearer}"));
            }
            let reply = send(&app, request.body(Body::empty()).unwrap()).await;
            assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
            assert_eq!(reply.header("allow"), "POST", "{method}");
        }
    }
    assert_eq!(provider.calls(), 0, "no non-POST may reach the provider");
}

/// A chunked body has no `Content-Length` to refuse up front, so only a
/// limit that counts bytes stops it. Without the guard's, the whole body is
/// buffered and handed to the provider before `rmcp`'s own limit answers
/// the same 413.
#[tokio::test]
async fn an_oversized_chunked_body_is_413_before_the_provider_runs() {
    let tools = FakeTools::default();
    let (app, provider) = counted(&tools, RESOURCE);
    let request = post("/mcp", Some(&token("u-1")), &rpc("tools/list", json!({})))
        .body(Body::new(Chunked::new(5, 1024 * 1024)))
        .unwrap();

    let reply = send(&app, request).await;

    assert_eq!(
        reply.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        reply.text
    );
    assert_eq!(
        provider.calls(),
        0,
        "an oversized body must not be authenticated"
    );

    // Positive control: a normal request does reach the provider.
    let ok = send(
        &app,
        tool_call("/mcp", Some(&token("u-1")), "echo", json!({ "text": "a" })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(provider.calls(), 1);
}

/// Two `Authorization` values are ambiguous, even two identical valid ones:
/// the guard's token and the provider's could differ, and the guard's is
/// the one it redacts from the logs. RFC 6750 calls that shape
/// `invalid_request` (400), not a missing token (review on #1067).
#[tokio::test]
async fn two_authorization_headers_are_an_invalid_request() {
    let (app, provider) = counted(&FakeTools::default(), RESOURCE);
    let mut request = tool_call("/mcp", None, "echo", json!({ "text": "a" }));
    let value: http::HeaderValue = format!("Bearer {}", token("u-1")).parse().unwrap();
    request.headers_mut().append("authorization", value.clone());
    request.headers_mut().append("authorization", value);

    let reply = send(&app, request).await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply
            .header("www-authenticate")
            .contains("error=\"invalid_request\""),
        "{}",
        reply.header("www-authenticate")
    );
    assert_eq!(provider.calls(), 0);
}

/// `Bearer` with no token is malformed too; another scheme is simply no
/// bearer credentials, which keeps the error-free 401 challenge.
#[tokio::test]
async fn an_empty_bearer_is_invalid_and_another_scheme_is_missing() {
    let (app, provider) = counted(&FakeTools::default(), RESOURCE);
    let mut empty = tool_call("/mcp", None, "echo", json!({ "text": "a" }));
    empty
        .headers_mut()
        .insert("authorization", "Bearer   ".parse().unwrap());
    let reply = send(&app, empty).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(reply.header("www-authenticate").contains("invalid_request"));

    let mut basic = tool_call("/mcp", None, "echo", json!({ "text": "a" }));
    basic
        .headers_mut()
        .insert("authorization", "Basic dTpw".parse().unwrap());
    let reply = send(&app, basic).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert!(!reply.header("www-authenticate").contains("error="));
    assert_eq!(provider.calls(), 0);
}

/// The challenge is built from the identifier, never from what the request
/// claims its host is: a `Host` or `X-Forwarded-*` an attacker controls must
/// not point a client at another metadata document.
#[tokio::test]
async fn the_challenge_ignores_the_requests_host_headers() {
    let mut request = tool_call("/mcp", None, "echo", json!({ "text": "a" }));
    let headers = request.headers_mut();
    headers.insert("host", "evil.example".parse().unwrap());
    headers.insert("x-forwarded-host", "evil.example".parse().unwrap());
    headers.insert("x-forwarded-proto", "https".parse().unwrap());
    headers.insert(
        "forwarded",
        "host=evil.example;proto=https".parse().unwrap(),
    );

    let reply = send(&served(&FakeTools::default()), request).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.header("www-authenticate"),
        "Bearer resource_metadata=\"http://localhost/.well-known/oauth-protected-resource/mcp\""
    );
}

/// The allowed `Host` defaults to the identifier's authority. `rmcp`'s own
/// default is loopback only, which every other test here satisfies, so
/// only a deployed identifier tells the two apart.
#[tokio::test]
async fn the_allowed_host_defaults_to_the_identifiers_authority() {
    const DEPLOYED: &str = "https://api.example.test/mcp";
    let tools = FakeTools::default();
    let (app, _) = counted(&tools, DEPLOYED);
    let call = |host: &str| {
        let mut request = tool_call(
            "/mcp",
            Some(&mint(DEPLOYED, json!({ "id": "u-1" }))),
            "echo",
            json!({ "text": "a" }),
        );
        request.headers_mut().insert("host", host.parse().unwrap());
        request
    };

    let deployed = send(&app, call("api.example.test")).await;
    assert_eq!(deployed.status, StatusCode::OK, "{}", deployed.text);
    let loopback = send(&app, call("localhost")).await;
    assert_eq!(loopback.status, StatusCode::FORBIDDEN, "{}", loopback.text);
    assert_eq!(tools.runs(), 1);
}

/// 2026-07-28 has no sessions. A legacy `initialize` is answered like any
/// other request from a version this server does not speak, and no session
/// is ever minted. With `rmcp`'s legacy session mode on, it would try to
/// open one instead.
#[tokio::test]
async fn a_legacy_initialize_opens_no_session() {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "0" },
        },
    });
    let mut request = post("/mcp", Some(&token("u-1")), &body)
        .body(Body::from(body.to_string()))
        .unwrap();
    request
        .headers_mut()
        .insert("mcp-protocol-version", "2025-11-25".parse().unwrap());

    let reply = send(&served(&FakeTools::default()), request).await;

    assert!(
        reply.headers.get("mcp-session-id").is_none(),
        "{:?}",
        reply.headers
    );
    assert_eq!(
        reply.json()["error"]["code"],
        json!(-32022),
        "{}",
        reply.text
    );
}
