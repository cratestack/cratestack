//! `listed` fails closed on an HTTP server without the guard's caller, and
//! answers once the guard has attached one; and `server.rs` sends all three
//! resource methods through that check. Over the wire the guard always
//! attaches a caller, so only a test below the guard sees the refusal.

use bytes::Bytes;
use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind, Value};
use http_body_util::{BodyExt, Full};
use rmcp::model::{ErrorCode, Extensions};
use serde_json::json;

use super::{ResourceDescriptor, list_resources, list_templates, listed};
use crate::server::McpServer;
use crate::streamable_http::caller::{Caller, hand_over};
use crate::table::{ArgumentsError, McpTools, ToolDescriptor};
use crate::{ProtectedResource, StreamableHttpServer};

static READ: OpDescriptor = OpDescriptor {
    op_id: "model.Post.get",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: true,
    auth_required: false,
};
static RESOURCES: [ResourceDescriptor; 1] =
    [ResourceDescriptor::new("blog", "posts", 200, &READ, &READ)];

enum NoCall {}

struct Table;

impl McpTools for Table {
    type Call = NoCall;

    fn tools(&self) -> &'static [ToolDescriptor] {
        &[]
    }

    fn decode(&self, tool: &str, _: serde_json::Value) -> Result<NoCall, ArgumentsError> {
        Err(ArgumentsError::new(format!("no tool `{tool}`")))
    }

    async fn execute(
        &self,
        call: NoCall,
        _: &CratestackContext,
    ) -> Result<serde_json::Value, CratestackError> {
        match call {}
    }

    fn resources(&self) -> &'static [ResourceDescriptor] {
        &RESOURCES
    }
}

#[test]
fn both_lists_refuse_an_http_request_without_the_guards_caller() {
    let server = McpServer::with_caller(Table, Caller::PerRequest).unwrap();
    let unauthenticated = Extensions::new();

    let error = listed(&server, &unauthenticated, list_resources).unwrap_err();
    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    let error = listed(&server, &unauthenticated, list_templates).unwrap_err();
    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);

    let (mut parts, ()) = http::Request::new(()).into_parts();
    let caller = CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))]);
    hand_over(&mut parts, caller);
    let mut authenticated = Extensions::new();
    authenticated.insert(parts);

    let resources = listed(&server, &authenticated, list_resources).unwrap();
    assert_eq!(resources.resources.len(), 1);
    let templates = listed(&server, &authenticated, list_templates).unwrap();
    assert_eq!(templates.resource_templates.len(), 2);
}

/// The test above pins `listed`; this one pins that `server.rs` routes the
/// three resource methods through a caller check at all. Bypassing `listed`
/// in `list_resources` passed every suite, because over the wire the guard
/// always attaches a caller and the lists do not depend on it. So this
/// hands each request to `rmcp` *behind* the guard, as a layer mounted
/// around it by mistake would, and requires the fail-closed error. The
/// same request with the guard's caller attached must succeed, so the
/// refusal is the caller check's and not `rmcp` rejecting the request.
#[tokio::test]
async fn every_resource_method_refuses_a_request_that_skipped_the_guard() {
    let provider = |_: &http::HeaderMap| Ok::<_, CratestackError>(CratestackContext::anonymous());
    let resource = ProtectedResource::new("http://localhost/mcp", ["https://auth.example.test"]);
    let http =
        StreamableHttpServer::builder(Table, provider, ["https://app.example.test"], resource)
            .build()
            .unwrap();
    let inner = &http.service().shared.inner;

    for (method, params) in [
        ("resources/list", json!({})),
        ("resources/templates/list", json!({})),
        (
            "resources/read",
            json!({ "uri": "cratestack://blog/posts/1" }),
        ),
    ] {
        let request = |caller: Option<CratestackContext>| {
            let mut params = params.clone();
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
            if let Some(uri) = params["uri"].as_str() {
                builder = builder.header("mcp-name", uri);
            }
            let (mut parts, body) = builder
                .body(Full::new(Bytes::from(body.to_string())))
                .unwrap()
                .into_parts();
            if let Some(caller) = caller {
                hand_over(&mut parts, caller);
            }
            http::Request::from_parts(parts, body)
        };
        let answer = |reply: http::Response<_>| async move {
            let bytes = BodyExt::collect(reply).await.unwrap().to_bytes();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        };

        let unguarded = answer(inner.handle(request(None)).await).await;
        assert_eq!(
            unguarded["error"]["code"],
            json!(-32603),
            "{method}: {unguarded}"
        );
        assert_eq!(
            unguarded["error"]["message"], "no authenticated caller for this request",
            "{method}: {unguarded}"
        );

        let caller =
            CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))]);
        let guarded = answer(inner.handle(request(Some(caller))).await).await;
        // The request shape is one `rmcp` serves: the lists answer, and the
        // read reaches the table (which has no rows) and gets not-found.
        if method == "resources/read" {
            assert_eq!(
                guarded["error"]["message"], "resource not found",
                "{guarded}"
            );
        } else {
            assert!(guarded.get("result").is_some(), "{method}: {guarded}");
        }
    }
}
