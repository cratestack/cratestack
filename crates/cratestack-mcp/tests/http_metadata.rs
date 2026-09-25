//! cratestack#1039: RFC 9728 Protected Resource Metadata, a mount under
//! `Router::nest` (the shape the rate limiter got wrong once, cratestack#877
//! era: a path read from the request is the path *after* the nest stripped
//! its prefix), and the configurations the builder refuses.

mod support;

use axum::Router;
use axum::body::Body;
use cratestack_mcp::{HttpConfigError, ProtectedResource, StreamableHttpServer};
use http::{Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::http_app::{APP_ORIGIN, RESOURCE, builder, send, served, token, tool_call};
use support::token::{AudienceProvider, ISSUER, mint};

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("host", "localhost")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn metadata_is_served_at_the_suffixed_and_the_root_path() {
    let tools = FakeTools::default();
    let resource = ProtectedResource::new(RESOURCE, [ISSUER, "https://backup.example.test"])
        .with_scopes(["mcp:tools"]);
    let server = StreamableHttpServer::builder(
        tools,
        AudienceProvider::new(RESOURCE),
        [APP_ORIGIN],
        resource,
    )
    .build()
    .unwrap();
    let app = Router::new()
        .nest_service("/mcp", server.service())
        .merge(server.metadata_router());

    let expected = json!({
        "resource": RESOURCE,
        "authorization_servers": [ISSUER, "https://backup.example.test"],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp:tools"],
    });
    for path in [
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-protected-resource",
    ] {
        let reply = send(&app, get(path)).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.header("content-type"), "application/json", "{path}");
        assert_eq!(reply.json(), expected, "{path}");
    }
    assert_eq!(
        server.resource_metadata_url(),
        "http://localhost/.well-known/oauth-protected-resource/mcp"
    );
}

/// The endpoint nested two levels deep (`/api/v1/mcp`), the metadata merged
/// at the root. The document, the challenge and a real call must all name
/// `/api/v1/mcp`, which the service itself never sees: `nest` hands it `/`.
#[tokio::test]
async fn a_nested_mount_is_described_and_served_under_its_full_path() {
    const NESTED: &str = "http://localhost/api/v1/mcp";
    let tools = FakeTools::default();
    let server = builder(&tools, NESTED).build().unwrap();
    let app = Router::new()
        .nest(
            "/api",
            Router::new().nest("/v1", Router::new().nest_service("/mcp", server.service())),
        )
        .merge(server.metadata_router());

    let document = send(
        &app,
        get("/.well-known/oauth-protected-resource/api/v1/mcp"),
    )
    .await;
    assert_eq!(document.status, StatusCode::OK);
    assert_eq!(document.json()["resource"], NESTED);

    let challenged = send(
        &app,
        tool_call("/api/v1/mcp", None, "echo", json!({ "text": "a" })),
    )
    .await;
    assert_eq!(challenged.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        challenged.header("www-authenticate"),
        "Bearer resource_metadata=\"http://localhost/.well-known/oauth-protected-resource/api/v1/mcp\""
    );

    let nested_token = mint(NESTED, json!({ "id": "u-1" }));
    let call = tool_call(
        "/api/v1/mcp",
        Some(&nested_token),
        "echo",
        json!({ "text": "a" }),
    );
    let reply = send(&app, call).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.result()["structuredContent"], json!({ "text": "a" }));

    // A token for the un-nested identifier is for another resource.
    let wrong = tool_call(
        "/api/v1/mcp",
        Some(&token("u-1")),
        "echo",
        json!({ "text": "a" }),
    );
    assert_eq!(send(&app, wrong).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(tools.runs(), 1);
}

/// A host other than the resource identifier's is `rmcp`'s DNS-rebinding
/// refusal, which the builder points at the identifier by default.
#[tokio::test]
async fn a_foreign_host_is_refused() {
    let mut request = tool_call("/mcp", Some(&token("u-1")), "echo", json!({ "text": "a" }));
    request
        .headers_mut()
        .insert("host", "attacker.example".parse().unwrap());
    let reply = send(&served(&FakeTools::default()), request).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
}

fn build(origins: &[&str], resource: ProtectedResource) -> Result<(), HttpConfigError> {
    StreamableHttpServer::builder(
        FakeTools::default(),
        AudienceProvider::new(RESOURCE),
        origins.iter().copied(),
        resource,
    )
    .build()
    .map(drop)
}

#[test]
fn the_builder_refuses_what_would_be_a_silent_hole() {
    let ok = || ProtectedResource::new(RESOURCE, [ISSUER]);
    assert_eq!(build(&[], ok()), Err(HttpConfigError::NoAllowedOrigins));
    assert_eq!(
        build(&["*"], ok()),
        Err(HttpConfigError::InvalidOrigin("*".to_owned()))
    );
    assert_eq!(
        build(
            &[APP_ORIGIN],
            ProtectedResource::new(RESOURCE, Vec::<String>::new())
        ),
        Err(HttpConfigError::NoAuthorizationServers)
    );
    for resource in [
        "/mcp",
        "localhost/mcp",
        "ftp://localhost/mcp",
        "http://localhost/mcp?x=1",
        "http://localhost/mcp#frag",
        "http://user@localhost/mcp",
        "http://localhost/{tool}",
    ] {
        assert_eq!(
            build(&[APP_ORIGIN], ProtectedResource::new(resource, [ISSUER])),
            Err(HttpConfigError::InvalidResource(resource.to_owned())),
            "{resource}"
        );
    }
    assert_eq!(
        build(
            &[APP_ORIGIN],
            ProtectedResource::new(RESOURCE, ["not a url"])
        ),
        Err(HttpConfigError::InvalidAuthorizationServer(
            "not a url".to_owned()
        ))
    );
    assert_eq!(
        build(&[APP_ORIGIN], ok().with_scopes(["two words"])),
        Err(HttpConfigError::InvalidScope("two words".to_owned()))
    );
    assert_eq!(build(&[APP_ORIGIN, "null"], ok()), Ok(()));
}
