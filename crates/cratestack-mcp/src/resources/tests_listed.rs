//! `listed` fails closed on an HTTP server without the guard's caller, and
//! answers once the guard has attached one. Over the wire the guard always
//! attaches one, so only a test at this level sees the refusal.

use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind, Value};
use rmcp::model::{ErrorCode, Extensions};

use super::{ResourceDescriptor, list_resources, list_templates, listed};
use crate::server::McpServer;
use crate::streamable_http::caller::{Caller, hand_over};
use crate::table::{ArgumentsError, McpTools, ToolDescriptor};

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
