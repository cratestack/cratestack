//! One `tools/call`, in ADR 0002 § Dispatch's order:
//!
//! ```text
//! look up the tool            unknown          -> JSON-RPC -32602
//! decode the arguments        does not decode  -> isError, names the field
//! L3 admission                rate limit, then idempotency (src/admission.rs)
//! execute                     generated invoke_with_db, policy first
//! ```
//!
//! Only the first step is a protocol error. Everything after it is an
//! `isError` result, so an agent can read what went wrong and correct
//! itself (ADR 0002 § Tools, "Errors").

use cratestack_core::CratestackError;
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, CallToolResult, RequestMetaObject};
use serde_json::Value;

use crate::admission::admit_and_run;
use crate::idempotency::idempotency_key;
use crate::result::failure;
use crate::server::McpServer;
use crate::table::McpTools;

pub(crate) async fn call_tool<T: McpTools>(
    server: &McpServer<T>,
    request: CallToolRequestParams,
    request_meta: &RequestMetaObject,
) -> Result<CallToolResult, ErrorData> {
    let Some(descriptor) = server
        .tools
        .tools()
        .iter()
        .find(|descriptor| descriptor.name == request.name)
    else {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "mcp_tool_call",
            cratestack_tool = %request.name,
            "unknown MCP tool",
        );
        // -32602, which 2026-07-28 names for an unknown tool. The name is
        // the caller's own input, so echoing it reveals nothing.
        return Err(ErrorData::invalid_params(
            format!("unknown tool `{}`", request.name),
            None,
        ));
    };

    // A missing `arguments` is an empty object: a tool whose `Args` has no
    // required field accepts it, and one that has any names the field.
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let call = match server.tools.decode(descriptor.name, arguments.clone()) {
        Ok(call) => call,
        Err(error) => {
            return Ok(failure(
                descriptor,
                CratestackError::Validation(error.to_string()),
            ));
        }
    };

    // `rmcp` lifts `params._meta` into the request context while decoding,
    // so that is where the key is; `request.meta` is read too, for a
    // request built in-process without going through the wire decoder.
    let key = match idempotency_key(request_meta, request.meta.as_ref()) {
        Ok(key) => key,
        Err(error) => return Ok(failure(descriptor, error)),
    };

    Ok(admit_and_run(server, descriptor, &arguments, key.as_deref(), call).await)
}
