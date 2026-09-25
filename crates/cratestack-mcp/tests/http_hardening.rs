//! cratestack#1039 review: requests the guard let through on `79c7984c`.
//!
//! - A second `Mcp-Method` / `Mcp-Name` / `MCP-Protocol-Version` value.
//!   `rmcp` validates only the first value of each against the body, and an
//!   intermediary that routes or rate-limits on the header may read another,
//!   which is exactly the split MCP's header validation exists to prevent
//!   ("different components rely on different sources of truth").
//! - A token in the query string. MCP forbids it and OAuth 2.1 dropped the
//!   query method; an application provider shared with REST may still read
//!   `access_token` there, and the guard strips only `Authorization`, so the
//!   token would travel on to `rmcp` and the handler in the request URI.
//! - An allowed origin written with its default port (`:443`). The guard
//!   admitted the browser form (`https://host`) and `rmcp`'s second check
//!   then refused it, so every browser request from that origin got 403.

mod support;

use axum::body::Body;
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::StatusCode;
use serde_json::json;
use support::FakeTools;
use support::counting::CountingProvider;
use support::http_app::{RESOURCE, mount, post, rpc, send, served, token, tool_call};
use support::token::{AudienceProvider, ISSUER};

fn echo_call() -> http::Request<Body> {
    tool_call("/mcp", Some(&token("u-1")), "echo", json!({ "text": "a" }))
}

#[tokio::test]
async fn a_second_value_of_a_mirrored_mcp_header_is_a_header_mismatch() {
    let tools = FakeTools::default();
    let app = served(&tools);
    let control = send(&app, echo_call()).await;
    assert_eq!(control.status, StatusCode::OK, "{}", control.text);

    for (name, second) in [
        ("mcp-name", "transfer"),
        ("mcp-name", "echo"),
        ("mcp-method", "tools/list"),
        ("mcp-protocol-version", "2026-07-28"),
    ] {
        let mut request = echo_call();
        request.headers_mut().append(name, second.parse().unwrap());

        let reply = send(&app, request).await;

        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{name}: {}",
            reply.text
        );
        assert_eq!(
            reply.json()["error"]["code"],
            json!(-32020),
            "{name}: {}",
            reply.text
        );
    }
    assert_eq!(tools.runs(), 1, "only the control ran");
}

#[tokio::test]
async fn a_second_mcp_param_value_is_a_header_mismatch() {
    let tools = FakeTools::default();
    let mut request = echo_call();
    request
        .headers_mut()
        .append("mcp-param-region", "a".parse().unwrap());
    request
        .headers_mut()
        .append("mcp-param-region", "b".parse().unwrap());

    let reply = send(&served(&tools), request).await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(
        reply.json()["error"]["code"],
        json!(-32020),
        "{}",
        reply.text
    );
    assert_eq!(tools.runs(), 0);
}

/// RFC 6750 §3.1 `invalid_request`: a token presented in more than one way,
/// or in a way this resource does not accept. Refused before the provider
/// could read it, with or without a header token beside it, and however the
/// parameter name is percent-encoded.
#[tokio::test]
async fn a_token_in_the_query_string_is_refused_before_the_provider_runs() {
    let tools = FakeTools::default();
    let provider = CountingProvider::new(RESOURCE);
    let server = StreamableHttpServer::builder(
        tools.clone(),
        provider.clone(),
        [support::http_app::APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    )
    .build()
    .unwrap();
    let app = mount(&server);
    let valid = token("u-1");

    for (query, header) in [
        (format!("access_token={valid}"), Some(valid.as_str())),
        (format!("x=1&access_token={valid}"), None),
        (format!("access%5Ftoken={valid}"), Some(valid.as_str())),
    ] {
        let body = rpc(
            "tools/call",
            json!({ "name": "echo", "arguments": { "text": "a" } }),
        );
        let request = post(&format!("/mcp?{query}"), header, &body)
            .body(Body::from(body.to_string()))
            .unwrap();

        let reply = send(&app, request).await;

        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{query}: {}",
            reply.text
        );
        assert!(
            reply
                .header("www-authenticate")
                .starts_with("Bearer error=\"invalid_request\""),
            "{}",
            reply.header("www-authenticate")
        );
        assert!(!reply.text.contains(&valid), "the token is not echoed");
    }
    assert_eq!(provider.calls(), 0);
    assert_eq!(tools.runs(), 0);

    // An unrelated query parameter is not a token.
    let body = rpc(
        "tools/call",
        json!({ "name": "echo", "arguments": { "text": "a" } }),
    );
    let request = post("/mcp?trace=1", Some(&valid), &body)
        .body(Body::from(body.to_string()))
        .unwrap();
    assert_eq!(send(&app, request).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_origin_listed_with_its_default_port_admits_what_browsers_send() {
    let tools = FakeTools::default();
    for listed in ["https://app.example.test:443", "http://localhost:80"] {
        let server = StreamableHttpServer::builder(
            tools.clone(),
            AudienceProvider::new(RESOURCE),
            [listed],
            ProtectedResource::new(RESOURCE, [ISSUER]),
        )
        .build()
        .unwrap();
        let browser_form = listed.rsplit_once(':').unwrap().0;
        let mut request = echo_call();
        request
            .headers_mut()
            .insert("origin", browser_form.parse().unwrap());

        let reply = send(&mount(&server), request).await;

        assert_eq!(reply.status, StatusCode::OK, "{listed}: {}", reply.text);
    }
    assert_eq!(tools.runs(), 2);
}
