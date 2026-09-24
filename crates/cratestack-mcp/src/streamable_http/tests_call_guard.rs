//! `call_tool` and `list_tools` fail closed when a request reaches `rmcp`
//! without the guard's caller. Over the wire the guard always attaches one,
//! so a `resolve(...).unwrap_or(anonymous)` in `server.rs` passed every
//! suite (phase 5 review, mutation M18). This hands a `tools/call` or a
//! `tools/list` to the `rmcp` service *behind* the guard — as a layer
//! mounted around it by mistake would — and requires the fail-closed error;
//! the same request with the guard's caller attached must succeed, so the
//! refusal is the caller check's and not `rmcp` rejecting the request's
//! shape.

use bytes::Bytes;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, OpDescriptor, OpKind, Value,
};
use http_body_util::{BodyExt, Full};
use serde_json::json;

use super::builder::StreamableHttp;
use super::caller::hand_over;
use crate::table::{ArgumentsError, McpTools, ToolDescriptor};
use crate::{ProtectedResource, StreamableHttpServer};

static OP: OpDescriptor = OpDescriptor {
    op_id: "procedure.whoami",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: false,
    auth_required: true,
};
static TOOLS: [ToolDescriptor; 1] = [ToolDescriptor::new(
    "whoami",
    None,
    r#"{"type":"object"}"#,
    None,
    true,
    &OP,
)];

struct Table;

impl McpTools for Table {
    type Call = ();

    fn tools(&self) -> &'static [ToolDescriptor] {
        &TOOLS
    }

    fn decode(&self, tool: &str, _: serde_json::Value) -> Result<(), ArgumentsError> {
        match tool {
            "whoami" => Ok(()),
            other => Err(ArgumentsError::new(format!("no tool `{other}`"))),
        }
    }

    async fn execute(
        &self,
        (): (),
        ctx: &CratestackContext,
    ) -> Result<serde_json::Value, CratestackError> {
        Ok(json!({ "id": ctx.principal_actor_id() }))
    }
}

fn call(caller: Option<CratestackContext>) -> http::Request<Full<Bytes>> {
    let params = json!({ "name": "whoami", "arguments": {} });
    request("tools/call", params, Some("whoami"), caller)
}

fn request(
    method: &str,
    mut params: serde_json::Value,
    mcp_name: Option<&str>,
    caller: Option<CratestackContext>,
) -> http::Request<Full<Bytes>> {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
    });
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let mut builder = http::Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method);
    if let Some(name) = mcp_name {
        builder = builder.header("mcp-name", name);
    }
    let (mut parts, body) = builder
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
        .into_parts();
    if let Some(caller) = caller {
        hand_over(&mut parts, caller);
    }
    http::Request::from_parts(parts, body)
}

fn server() -> StreamableHttp<Table, impl AuthProvider> {
    let provider = |_: &http::HeaderMap| Ok::<_, CratestackError>(CratestackContext::anonymous());
    let resource = ProtectedResource::new("http://localhost/mcp", ["https://auth.example.test"]);
    StreamableHttpServer::builder(Table, provider, ["https://app.example.test"], resource)
        .build()
        .unwrap()
}

async fn answer<B>(reply: http::Response<B>) -> serde_json::Value
where
    B: http_body::Body,
    B::Error: std::fmt::Debug,
{
    let bytes = BodyExt::collect(reply).await.unwrap().to_bytes();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
}

fn user() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))])
}

/// `tools/list` resolves the caller too (maintainer decision on #1040), so
/// every list method fails closed without the guard's caller, as the
/// resource lists do (`resources::listed`). The list is static, so this is
/// defence in depth: a way around the guard is refused on every method, not
/// only the ones whose answer depends on who asks.
#[tokio::test]
async fn a_tool_list_that_skipped_the_guard_fails_closed() {
    let http = server();
    let inner = &http.service().shared.inner;

    let unguarded = answer(
        inner
            .handle(request("tools/list", json!({}), None, None))
            .await,
    )
    .await;
    assert_eq!(unguarded["error"]["code"], json!(-32603), "{unguarded}");
    assert_eq!(
        unguarded["error"]["message"], "no authenticated caller for this request",
        "{unguarded}"
    );

    let guarded = request("tools/list", json!({}), None, Some(user()));
    let guarded = answer(inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"]["tools"][0]["name"],
        json!("whoami"),
        "{guarded}"
    );
}

#[tokio::test]
async fn a_tool_call_that_skipped_the_guard_fails_closed() {
    let http = server();
    let inner = &http.service().shared.inner;

    let unguarded = answer(inner.handle(call(None)).await).await;
    assert_eq!(unguarded["error"]["code"], json!(-32603), "{unguarded}");

    let guarded = answer(inner.handle(call(Some(user()))).await).await;
    // No output schema, so the result is text only (no `structuredContent`).
    assert_eq!(guarded["result"]["isError"], json!(false), "{guarded}");
    assert_eq!(
        guarded["result"]["content"][0]["text"],
        json!(r#"{"id":"u-1"}"#),
        "{guarded}"
    );
}
