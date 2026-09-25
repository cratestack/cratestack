//! `call_tool` fails closed when a request reaches `rmcp` without the
//! guard's caller. Over the wire the guard always attaches one, so a
//! `resolve(...).unwrap_or(anonymous)` in `server.rs` passed every suite
//! (phase 5 review, mutation M18). This hands a `tools/call` to the `rmcp`
//! service *behind* the guard — as a layer mounted around it by mistake
//! would — and requires the fail-closed error; the same request with the
//! guard's caller attached must run, so the refusal is the caller check's
//! and not `rmcp` rejecting the request's shape.

use bytes::Bytes;
use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind, Value};
use http_body_util::{BodyExt, Full};
use serde_json::json;

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

fn request(caller: Option<CratestackContext>) -> http::Request<Full<Bytes>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "whoami",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
            },
        },
    });
    let (mut parts, body) = http::Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call")
        .header("mcp-name", "whoami")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
        .into_parts();
    if let Some(caller) = caller {
        hand_over(&mut parts, caller);
    }
    http::Request::from_parts(parts, body)
}

#[tokio::test]
async fn a_tool_call_that_skipped_the_guard_fails_closed() {
    let provider = |_: &http::HeaderMap| Ok::<_, CratestackError>(CratestackContext::anonymous());
    let resource = ProtectedResource::new("http://localhost/mcp", ["https://auth.example.test"]);
    let http =
        StreamableHttpServer::builder(Table, provider, ["https://app.example.test"], resource)
            .build()
            .unwrap();
    let inner = &http.service().shared.inner;
    let answer = |reply: http::Response<_>| async move {
        let bytes = BodyExt::collect(reply).await.unwrap().to_bytes();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };

    let unguarded = answer(inner.handle(request(None)).await).await;
    assert_eq!(unguarded["error"]["code"], json!(-32603), "{unguarded}");

    let caller = CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))]);
    let guarded = answer(inner.handle(request(Some(caller))).await).await;
    // No output schema, so the result is text only (no `structuredContent`).
    assert_eq!(guarded["result"]["isError"], json!(false), "{guarded}");
    assert_eq!(
        guarded["result"]["content"][0]["text"],
        json!(r#"{"id":"u-1"}"#),
        "{guarded}"
    );
}
