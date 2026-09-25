//! cratestack#1039 (MCP phase 4): the generated `cratestack_schema::mcp`
//! table served over Streamable HTTP, behind an application `AuthProvider`.
//!
//! The protocol-level behaviour (Origin, 401, metadata, 405, header
//! mismatch, token hygiene) is tested against a hand-written table in
//! `cratestack-mcp`'s `tests/http_*.rs`. What only the generated table can
//! show is here: the context the provider builds is the one the procedure's
//! generated policy check and implementation receive, and a real MCP client
//! can make a call end to end.
//!
//! Gated `required-features = ["mcp"]`; `just test-ci-host` runs it.

mod mcp_http_support;

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::mcp::McpTools;
use cratestack::mcp::{ProtectedResource, StreamableHttp, StreamableHttpServer};
use http_body_util::BodyExt;
use mcp_http_support::token::{AudienceProvider, ISSUER, mint};
use mcp_http_support::{Registry, tools};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_ORIGIN: &str = "https://app.example.test";

fn server<T: McpTools>(tools: T, resource: &str) -> StreamableHttp<T, AudienceProvider> {
    StreamableHttpServer::builder(
        tools,
        AudienceProvider::new(resource),
        [APP_ORIGIN],
        ProtectedResource::new(resource, [ISSUER]),
    )
    .build()
    .expect("valid configuration")
}

fn caller(role: &str) -> Value {
    json!({ "id": "u-1", "role": role, "tenant": "t-9" })
}

/// One `tools/call` of `me` over the service, as a conforming client sends
/// it; the JSON-RPC `result`.
async fn call_me(app: &Router, token: &str) -> Value {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "me",
            "arguments": { "tag": "x" },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "0" },
            },
        },
    });
    let request = http::Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call")
        .header("mcp-name", "me")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    reply["result"].clone()
}

/// The decisive context-propagation test. `me` is
/// `@allow(auth().role == "teller")`, and the token's `role` claim is the
/// only source of that field. A teller's call runs and returns exactly the
/// claims the provider put in the context; a clerk's is refused by the
/// generated policy and never runs. Serving the call under any context but
/// the provider's (an anonymous one, say) makes the teller's call fail.
#[tokio::test]
async fn the_procedure_runs_under_exactly_the_providers_context() {
    const RESOURCE: &str = "http://localhost/mcp";
    let registry = Registry::default();
    let http = server(tools(&registry), RESOURCE);
    let app = Router::new().nest_service("/mcp", http.service());

    let teller = call_me(&app, &mint(RESOURCE, caller("teller"))).await;
    assert_eq!(teller["isError"], json!(false), "{teller}");
    assert_eq!(teller["structuredContent"], caller("teller"));
    assert_eq!(registry.runs(), 1);

    let clerk = call_me(&app, &mint(RESOURCE, caller("clerk"))).await;
    assert_eq!(clerk["isError"], json!(true), "{clerk}");
    let envelope: Value =
        serde_json::from_str(clerk["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["code"], "FORBIDDEN");
    assert_eq!(registry.runs(), 1, "a denied call must not run");
}

/// One tool call through `rmcp`'s own Streamable HTTP client, over a real
/// socket, with a token the test provider verifies: discovery, listing and
/// the call all pass the guard, and a client without a token cannot start.
#[tokio::test]
async fn an_rmcp_client_calls_a_tool_with_a_real_token() {
    use rmcp::model::{CallToolRequestParams, ClientConfig, ProtocolVersion};
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
    use rmcp::{ClientLifecycleMode, ClientServiceExt};

    // `reqwest` is built with `rustls-no-provider`; the URL is plain HTTP.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let resource = format!("http://{}/mcp", listener.local_addr().unwrap());
    let registry = Registry::default();
    let http = server(tools(&registry), &resource);
    let app = Router::new()
        .nest_service("/mcp", http.service())
        .merge(http.metadata_router());
    let serving = tokio::spawn(async move {
        cratestack::axum::serve(listener, app).await.unwrap();
    });

    let connect = |token: Option<String>| {
        let mut config = StreamableHttpClientTransportConfig::with_uri(resource.as_str());
        if let Some(token) = token {
            config = config.auth_header(token);
        }
        ClientConfig::default().serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(config),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
    };

    assert!(
        connect(None).await.is_err(),
        "discovery without a token must be refused"
    );

    let client = connect(Some(mint(&resource, caller("teller"))))
        .await
        .expect("an authenticated client starts");
    let listed = client.list_all_tools().await.expect("tools/list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "me");

    let arguments = json!({ "tag": "e2e" }).as_object().unwrap().clone();
    let result = client
        .call_tool(CallToolRequestParams::new("me").with_arguments(arguments))
        .await
        .expect("tools/call");
    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(result.structured_content, Some(caller("teller")));
    assert_eq!(registry.runs(), 1);

    serving.abort();
}
