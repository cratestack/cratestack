//! cratestack#1039 review: requests the guard let through on `79c7984c`.
//!
//! - An allowed origin written with its default port (`:443`). The guard
//!   admitted the browser form (`https://host`) and `rmcp`'s second check
//!   then refused it, so every browser request from that origin got 403.

mod support;

use axum::body::Body;
use cratestack_mcp::{ProtectedResource, StreamableHttpServer};
use http::StatusCode;
use serde_json::json;
use support::FakeTools;
use support::http_app::{RESOURCE, mount, send, token, tool_call};
use support::token::{AudienceProvider, ISSUER};

fn echo_call() -> http::Request<Body> {
    tool_call("/mcp", Some(&token("u-1")), "echo", json!({ "text": "a" }))
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
