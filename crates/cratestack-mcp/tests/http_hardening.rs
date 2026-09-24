//! cratestack#1039 review: requests the guard let through on `79c7984c`.
//!
//! - A second `Mcp-Method` / `Mcp-Name` / `MCP-Protocol-Version` value.
//!   `rmcp` validates only the first value of each against the body, and an
//!   intermediary that routes or rate-limits on the header may read another,
//!   which is exactly the split MCP's header validation exists to prevent
//!   ("different components rely on different sources of truth").
//! - An allowed origin written with its default port (`:443`). The guard
//!   admitted the browser form (`https://host`) and `rmcp`'s second check
//!   then refused it, so every browser request from that origin got 403.

mod support;

use axum::body::Body;
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::StatusCode;
use serde_json::json;
use support::FakeTools;
use support::http_app::{RESOURCE, mount, send, served, token, tool_call};
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
