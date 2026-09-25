//! cratestack#1039: what the Streamable HTTP guard refuses before MCP
//! handling — a foreign `Origin` (403), a method other than `POST` (405), a
//! missing or bad token (401 with the RFC 9728 challenge), a token for
//! another audience (401) — and that a good request does get through.
//!
//! Every refusal also asserts that the tool never ran: a status code alone
//! would pass for a request that ran and then failed.

mod support;

use axum::body::Body;
use cratestack_core::{CratestackContext, CratestackError};
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::{Method, Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::http_app::{APP_ORIGIN, RESOURCE, mount, post, rpc, send, served, token, tool_call};
use support::token::{ISSUER, mint, mint_raw};

const METADATA_URL: &str = "http://localhost/.well-known/oauth-protected-resource/mcp";

fn with_origin(mut request: Request<Body>, origin: &str) -> Request<Body> {
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request
}

/// The decisive Origin test. The token is valid and the request is a
/// well-formed call, so the Origin is the only reason to refuse it. With
/// the guard's own check removed, `rmcp`'s (enabled by the builder) still
/// refuses it; with both removed the call runs and this test fails.
#[tokio::test]
async fn a_foreign_origin_is_forbidden_even_with_a_valid_token() {
    let tools = FakeTools::default();
    let app = served(&tools);
    let request = tool_call("/mcp", Some(&token("u-1")), "echo", json!({ "text": "hi" }));

    let reply = send(&app, with_origin(request, "https://evil.example")).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert_eq!(tools.runs(), 0, "a refused call must not run");
}

/// The guard's check runs before authentication and before any MCP
/// handling: a foreign page gets 403, not the 401 challenge that would
/// tell it where to get a token. (`rmcp`'s own check runs after the
/// guard, so it cannot produce this answer.)
#[tokio::test]
async fn a_foreign_origin_is_refused_before_authentication() {
    let tools = FakeTools::default();
    let request = tool_call("/mcp", None, "echo", json!({ "text": "hi" }));

    let reply = send(
        &served(&tools),
        with_origin(request, "https://evil.example"),
    )
    .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert!(reply.headers.get("www-authenticate").is_none());
    assert_eq!(reply.json()["code"], "FORBIDDEN");
}

/// Positive controls for the two tests above: the listed origin, and no
/// origin at all (a non-browser client), both get through.
#[tokio::test]
async fn the_allowed_origin_and_no_origin_are_served() {
    let tools = FakeTools::default();
    let app = served(&tools);
    let token = token("u-1");

    let listed = with_origin(
        tool_call("/mcp", Some(&token), "echo", json!({ "text": "a" })),
        APP_ORIGIN,
    );
    let reply = send(&app, listed).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.result()["structuredContent"], json!({ "text": "a" }));

    let bare = tool_call("/mcp", Some(&token), "echo", json!({ "text": "b" }));
    assert_eq!(send(&app, bare).await.status, StatusCode::OK);
    assert_eq!(tools.runs(), 2);
}

#[tokio::test]
async fn get_and_delete_are_method_not_allowed() {
    let app = served(&FakeTools::default());
    for method in [Method::GET, Method::DELETE] {
        let request = Request::builder()
            .method(method.clone())
            .uri("/mcp")
            .header("host", "localhost")
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", token("u-1")))
            .body(Body::empty())
            .unwrap();
        let reply = send(&app, request).await;
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(reply.header("allow"), "POST", "{method}");
    }
}

/// No token: 401, and the challenge says where to authenticate, with no
/// `error` (RFC 6750 §3.1: the client simply sent none).
#[tokio::test]
async fn a_request_without_a_token_gets_the_resource_metadata_challenge() {
    let tools = FakeTools::default();
    let reply = send(
        &served(&tools),
        tool_call("/mcp", None, "echo", json!({ "text": "hi" })),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer resource_metadata=\"{METADATA_URL}\"")
    );
    assert_eq!(reply.json()["code"], "UNAUTHORIZED");
    assert_eq!(tools.runs(), 0);
}

/// With scopes configured, the challenge names them too.
#[tokio::test]
async fn the_challenge_carries_the_configured_scopes() {
    let tools = FakeTools::default();
    let resource =
        ProtectedResource::new(RESOURCE, [ISSUER]).with_scopes(["mcp:tools", "mcp:read"]);
    let server = StreamableHttpServer::builder(
        tools.clone(),
        support::token::AudienceProvider::new(RESOURCE),
        [APP_ORIGIN],
        resource,
    )
    .build()
    .unwrap();

    let bad = tool_call("/mcp", Some("not-a-token"), "echo", json!({ "text": "x" }));
    let reply = send(&mount(&server), bad).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.header("www-authenticate"),
        format!(
            "Bearer error=\"invalid_token\", resource_metadata=\"{METADATA_URL}\", \
             scope=\"mcp:tools mcp:read\""
        )
    );
}

/// The decisive audience test (ADR 0002 phase 4). The token is genuine —
/// right key, right issuer, not expired — but minted for another resource.
/// The example provider's `aud` check is the only thing between it and the
/// tool; removing that check makes this fail.
#[tokio::test]
async fn a_token_for_another_audience_is_refused() {
    let tools = FakeTools::default();
    let foreign = mint("http://other.example/mcp", json!({ "id": "u-1" }));

    let reply = send(
        &served(&tools),
        tool_call("/mcp", Some(&foreign), "echo", json!({ "text": "hi" })),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert!(
        reply
            .header("www-authenticate")
            .starts_with("Bearer error=\"invalid_token\""),
        "{}",
        reply.header("www-authenticate")
    );
    assert_eq!(tools.runs(), 0);
}

#[tokio::test]
async fn a_forged_expired_or_anonymous_token_is_refused() {
    let tools = FakeTools::default();
    let app = served(&tools);
    let expired = mint_raw(&json!({ "iss": ISSUER, "aud": RESOURCE, "exp": 1, "id": "u-1" }));
    let mut forged = token("u-1");
    forged.push('A');
    for bad in [expired, forged, "x".to_owned()] {
        let reply = send(
            &app,
            tool_call("/mcp", Some(&bad), "echo", json!({ "text": "a" })),
        )
        .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{bad}");
    }
    assert_eq!(tools.runs(), 0);

    // A provider that answers every request with an anonymous context, as
    // a REST provider may for a token it does not recognise, leaving the
    // rest to `@allow`. An MCP resource server must refuse it.
    let anonymous = StreamableHttpServer::builder(
        tools.clone(),
        |_: &http::HeaderMap| Ok::<_, CratestackError>(CratestackContext::anonymous()),
        [APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    )
    .build()
    .unwrap();
    let app = axum::Router::new().nest_service("/mcp", anonymous.service());
    let reply = send(
        &app,
        tool_call("/mcp", Some("any"), "echo", json!({ "text": "a" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(tools.runs(), 0);
}

/// `discover` goes through the same guard: nothing about the server is
/// answered to a caller without a token.
#[tokio::test]
async fn discovery_is_authenticated_too() {
    let body = rpc("server/discover", json!({}));
    let request = post("/mcp", None, &body)
        .body(Body::from(body.to_string()))
        .unwrap();
    let reply = send(&served(&FakeTools::default()), request).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);

    let request = post("/mcp", Some(&token("u-1")), &body)
        .body(Body::from(body.to_string()))
        .unwrap();
    let reply = send(&served(&FakeTools::default()), request).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.result()["supportedVersions"], json!(["2026-07-28"]));
}

fn forbidding(_: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    Err(CratestackError::Forbidden(
        "token lacks mcp:tools".to_owned(),
    ))
}

fn unavailable(_: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    Err(CratestackError::Unavailable("try again shortly".to_owned()))
}

fn broken(_: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    Err(CratestackError::Internal(
        "jwks endpoint down: 10.0.0.7".to_owned(),
    ))
}

/// How a provider's own refusals map. `Forbidden` (a valid token without
/// the scope) is RFC 6750's 403 `insufficient_scope`, naming the scopes. A
/// 5xx means the provider could not decide, so it passes through without a
/// challenge (a 401 would send the client to re-authenticate for nothing),
/// in REST's envelope: an `Internal` detail stays in the log, exactly as
/// REST keeps it (`Unavailable`'s message is public on REST too).
#[tokio::test]
async fn a_provider_forbidden_is_403_and_a_provider_outage_is_not_a_challenge() {
    let tools = FakeTools::default();
    let resource = || ProtectedResource::new(RESOURCE, [ISSUER]).with_scopes(["mcp:tools"]);
    let call = || tool_call("/mcp", Some("t"), "echo", json!({ "text": "a" }));

    let server = StreamableHttpServer::builder(tools.clone(), forbidding, [APP_ORIGIN], resource())
        .build()
        .unwrap();
    let reply = send(&mount(&server), call()).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(
        reply.header("www-authenticate"),
        format!(
            "Bearer error=\"insufficient_scope\", resource_metadata=\"{METADATA_URL}\", \
             scope=\"mcp:tools\""
        )
    );

    let server =
        StreamableHttpServer::builder(tools.clone(), unavailable, [APP_ORIGIN], resource())
            .build()
            .unwrap();
    let reply = send(&mount(&server), call()).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(reply.headers.get("www-authenticate").is_none());
    assert_eq!(reply.json()["code"], "UNAVAILABLE");

    let server = StreamableHttpServer::builder(tools.clone(), broken, [APP_ORIGIN], resource())
        .build()
        .unwrap();
    let reply = send(&mount(&server), call()).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(reply.headers.get("www-authenticate").is_none());
    assert_eq!(
        reply.json(),
        json!({ "code": "INTERNAL_ERROR", "message": "internal error", "details": null })
    );
    assert_eq!(tools.runs(), 0);
}
