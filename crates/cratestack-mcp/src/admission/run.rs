//! Running one admitted `tools/call` and rendering its outcome — split out
//! of `admission.rs` for the ~200-LoC ceiling (`CLAUDE.md`).

use cratestack_core::CratestackContext;
use rmcp::model::CallToolResult;

use crate::result::{failure, success};
use crate::server::McpServer;
use crate::table::{McpTools, ToolDescriptor};

/// A rendered outcome, the status its record carries, and whether it may be
/// recorded for replay at all (`CratestackError::is_idempotency_replayable`).
pub(super) struct Ran {
    pub(super) result: CallToolResult,
    pub(super) status: u16,
    pub(super) replayable: bool,
}

/// Execute, and render the outcome.
pub(super) async fn run<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    descriptor: &ToolDescriptor,
    call: T::Call,
) -> Ran {
    match server.tools.execute(call, ctx).await {
        Ok(value) => {
            tracing::info!(
                target: "cratestack",
                cratestack_operation = "mcp_tool_call",
                cratestack_tool = descriptor.name,
                "cratestack mcp tool call completed",
            );
            Ran {
                result: success(descriptor, value),
                status: 200,
                replayable: true,
            }
        }
        Err(error) => {
            let error = answered(descriptor, error);
            Ran {
                status: error.status_code().as_u16(),
                replayable: error.is_idempotency_replayable(),
                result: failure(descriptor, error),
            }
        }
    }
}

/// A `TRANSACTION_ABORTED` this tool's arm did not claim — another
/// procedure's, propagated by a body that may have committed work — is
/// answered as `INTERNAL_ERROR` and recorded, never as "nothing committed,
/// send it again" (`CratestackError::disowned_transaction_abort`,
/// docs/design/procedure-isolation.md §6).
fn answered(
    descriptor: &ToolDescriptor,
    error: cratestack_core::CratestackError,
) -> cratestack_core::CratestackError {
    let Some(internal) = error.disowned_transaction_abort() else {
        return error;
    };
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "mcp_tool_call",
        cratestack_tool = descriptor.name,
        cratestack_error = error.code(),
        cratestack_detail = internal.detail().unwrap_or(""),
        "a TRANSACTION_ABORTED the tool does not own is answered as INTERNAL_ERROR",
    );
    internal
}
