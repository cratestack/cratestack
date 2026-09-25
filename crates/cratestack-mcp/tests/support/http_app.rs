//! Drives the Streamable HTTP service through an axum `Router` with
//! `tower::ServiceExt::oneshot`: raw requests in, raw status, headers and
//! body out, so the assertions are about what a client on the wire sees.

use axum::Router;
use axum::body::Body;
use cratestack_mcp::{ProtectedResource, StreamableHttp, StreamableHttpServer};
use http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::FakeTools;
use super::client::meta;
use super::token::{AudienceProvider, ISSUER, mint};

/// The resource identifier of a server mounted at `/mcp`.
pub const RESOURCE: &str = "http://localhost/mcp";
/// The one browser origin the test servers allow.
pub const APP_ORIGIN: &str = "https://app.example.test";

pub type Server = StreamableHttp<FakeTools, AudienceProvider>;
pub type Builder = StreamableHttpServer<FakeTools, AudienceProvider>;

/// A builder whose provider only accepts tokens for `resource`.
pub fn builder(tools: &FakeTools, resource: &str) -> Builder {
    StreamableHttpServer::builder(
        tools.clone(),
        AudienceProvider::new(resource),
        [APP_ORIGIN],
        ProtectedResource::new(resource, [ISSUER]),
    )
}

/// The endpoint at `/mcp`, the metadata at the root.
pub fn mount<A: cratestack_core::AuthProvider>(server: &StreamableHttp<FakeTools, A>) -> Router {
    Router::new()
        .nest_service("/mcp", server.service())
        .merge(server.metadata_router())
}

pub fn served(tools: &FakeTools) -> Router {
    mount(
        &builder(tools, RESOURCE)
            .build()
            .expect("valid configuration"),
    )
}

/// A token for [`RESOURCE`] whose `id` claim is `id`.
pub fn token(id: &str) -> String {
    mint(RESOURCE, json!({ "id": id }))
}

/// A JSON-RPC request with the per-request `_meta` 2026-07-28 requires,
/// merged with any `_meta` already in `params`.
pub fn rpc(method: &str, mut params: Value) -> Value {
    let mut merged = meta("2026-07-28");
    if let Some(Value::Object(extra)) = params.as_object_mut().unwrap().remove("_meta") {
        merged.as_object_mut().unwrap().extend(extra);
    }
    params["_meta"] = merged;
    json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
}

/// A `POST` a conforming client would send for `body`: the `Mcp-Method`
/// and `Mcp-Name` headers mirror the body, as SEP-2243 requires.
pub fn post(path: &str, token: Option<&str>, body: &Value) -> http::request::Builder {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", body["method"].as_str().unwrap());
    if let Some(name) = body["params"]["name"].as_str() {
        request = request.header("mcp-name", name);
    }
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request
}

/// A complete `tools/call` request to `path`.
pub fn tool_call(path: &str, token: Option<&str>, name: &str, arguments: Value) -> Request<Body> {
    keyed_call(path, token, name, arguments, None)
}

/// A `tools/call` with an optional idempotency key in `_meta`.
pub fn keyed_call(
    path: &str,
    token: Option<&str>,
    name: &str,
    arguments: Value,
    key: Option<&str>,
) -> Request<Body> {
    let mut params = json!({ "name": name, "arguments": arguments });
    if let Some(key) = key {
        params["_meta"] = json!({ "dev.cratestack/idempotencyKey": key });
    }
    let body = rpc("tools/call", params);
    post(path, token, &body)
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text)
            .unwrap_or_else(|_| panic!("expected JSON, got {}: {}", self.status, self.text))
    }

    /// The `result` of a JSON-RPC answer (panics on a JSON-RPC error).
    pub fn result(&self) -> Value {
        let json = self.json();
        json.get("result")
            .cloned()
            .unwrap_or_else(|| panic!("expected a result, got {}: {json}", self.status))
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("no `{name}` header on a {}", self.status))
            .to_str()
            .unwrap()
    }
}

pub async fn send(router: &Router, request: Request<Body>) -> Reply {
    let response = router.clone().oneshot(request).await.expect("infallible");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}
