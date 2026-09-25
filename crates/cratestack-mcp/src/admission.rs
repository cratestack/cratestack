//! L3 admission for one decoded `tools/call`, then the run (ADR 0002 D2).
//!
//! Rate limiting first, then idempotency: the order an application gets on
//! HTTP by layering `RateLimitLayer` outside `IdempotencyLayer`, so a
//! throttled retry is refused before it can even read a replay. Both
//! questions go to the same `OpExecutor` REST and RPC ask; this file only
//! supplies what L3 leaves to the transport — the bucket key, the
//! idempotency namespace and fingerprint, and the results it renders.
//!
//! Every non-`match`ed outcome of the two `#[non_exhaustive]` admission
//! enums refuses, never runs, as `cratestack-axum`'s adapters do: a future
//! variant this build does not understand must not execute a mutation.

use cratestack_core::{CratestackContext, CratestackError, RateLimitDecision};
use cratestack_exec::{
    Admission, DEFAULT_STORE_TIMEOUT, OpAdmission, OpInput, RateLimitAdmission, RateLimitBucket,
};
use rmcp::model::CallToolResult;
use serde_json::Value;

use crate::fingerprint::fingerprint;
use crate::idempotency::{namespace, record, replay};
use crate::result::{failure, success};
use crate::server::McpServer;
use crate::table::{McpTools, ToolDescriptor};

pub(crate) async fn admit_and_run<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    descriptor: &ToolDescriptor,
    arguments: &Value,
    key: Option<&str>,
    call: T::Call,
) -> CallToolResult {
    let op = OpAdmission::from(descriptor.op);
    if let Err(error) = rate_limit(server, ctx, op).await {
        return failure(descriptor, error);
    }

    let key = match key {
        Some(key) if server.executor.idempotency_applies(&op) => key,
        // No key, no store, or an op that does not participate: `admit`
        // would answer `Bypass` for each, so no namespace is derived and
        // nothing is reserved (ADR 0002 Q6: "when absent, no reservation").
        _ => return run(server, ctx, descriptor, call).await.0,
    };
    let principal = match namespace(ctx) {
        Ok(principal) => principal,
        Err(error) => return failure(descriptor, error),
    };
    let input = OpInput::new(
        op,
        &principal,
        Some(key),
        fingerprint(descriptor.name, arguments),
    );
    match server.executor.admit(&input).await {
        Ok(Admission::Bypass) => run(server, ctx, descriptor, call).await.0,
        Ok(Admission::Reserved { token }) => {
            let (result, status) = run(server, ctx, descriptor, call).await;
            let (status, body) = record(&result, status);
            server
                .executor
                .complete(&principal, key, token, status, &[], &body)
                .await;
            result
        }
        Ok(Admission::Replay(recorded)) => {
            replay(&recorded).unwrap_or_else(|error| failure(descriptor, error))
        }
        Ok(Admission::InFlight) => failure(
            descriptor,
            CratestackError::Conflict(
                "another call with this idempotency key is still in flight".to_owned(),
            ),
        ),
        // Same code and wording as HTTP's `idempotency_key_conflict`.
        Ok(Admission::Conflict) => failure(
            descriptor,
            CratestackError::Validation(
                "idempotency_key_conflict: key reused with different arguments".to_owned(),
            ),
        ),
        Ok(_) => failure(
            descriptor,
            CratestackError::Internal(
                "idempotency: unhandled admission outcome; refusing rather than running the tool"
                    .to_owned(),
            ),
        ),
        // A failed idempotency store keeps failing the call: running a
        // mutation it could not reserve is the duplicate it exists to stop.
        Err(error) => failure(descriptor, error),
    }
}

/// Charge the caller's bucket, when the application configured a limiter
/// and the tool is not `@no_rate_limit`.
///
/// One store call is bounded at `DEFAULT_STORE_TIMEOUT` (500ms), HTTP's
/// default and for its reason: a Redis behind a connection manager with no
/// timeouts was measured hanging 19s per request during an outage, which
/// turned "degrade to unlimited" into a denial-of-service lever. An
/// elapsed budget is a transport-class failure (`Unavailable`). HTTP lets
/// the application tune the budget (`RateLimitLayer::with_store_timeout`);
/// MCP does not yet, and that is an API question left to the maintainer.
/// What a store failure then does is the application's
/// [`cratestack_exec::StoreErrorPolicy`], the same type HTTP takes.
async fn rate_limit<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    op: OpAdmission,
) -> Result<(), CratestackError> {
    if !server.executor.rate_limit_applies(&op) {
        return Ok(());
    }
    // One bucket per principal, named exactly like the idempotency
    // namespace (`src/idempotency.rs`) and refused without an identity for
    // the same reason.
    let bucket = namespace(ctx)?;
    let input = OpInput::for_rate_limit(op, RateLimitBucket::new(&bucket, None));
    let admitted = tokio::time::timeout(
        DEFAULT_STORE_TIMEOUT,
        server.executor.admit_rate_limit(&input),
    )
    .await
    .unwrap_or_else(|_elapsed| {
        Err(CratestackError::Unavailable(
            "rate limit store timed out".to_owned(),
        ))
    });
    match admitted {
        Ok(RateLimitAdmission::Bypass) => Ok(()),
        Ok(RateLimitAdmission::Consumed(outcome)) => match outcome.decision {
            RateLimitDecision::Allowed { .. } => Ok(()),
            // `TOO_MANY_REQUESTS`, the code REST's throttle carries.
            RateLimitDecision::Throttled { .. } => Err(CratestackError::TooManyRequests(
                "rate limit exceeded".to_owned(),
            )),
        },
        Ok(_) => Err(CratestackError::Internal(
            "rate limit: unhandled admission outcome; refusing rather than running the tool"
                .to_owned(),
        )),
        // The rule `RateLimitLayer` applies, read from the one shared
        // type: `Allow` (the default) serves through an unreachable store
        // (`Unavailable`) and refuses anything else; `Deny` refuses all.
        Err(error) if server.store_error_policy.permits(&error) => {
            tracing::warn!(
                target: "cratestack",
                cratestack_operation = "rate_limit",
                error = %error,
                policy = ?server.store_error_policy,
                "mcp: rate-limit store unavailable; serving the call unthrottled",
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Execute, and render the outcome with the status its record carries.
async fn run<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    descriptor: &ToolDescriptor,
    call: T::Call,
) -> (CallToolResult, u16) {
    match server.tools.execute(call, ctx).await {
        Ok(value) => {
            tracing::info!(
                target: "cratestack",
                cratestack_operation = "mcp_tool_call",
                cratestack_tool = descriptor.name,
                "cratestack mcp tool call completed",
            );
            (success(descriptor, value), 200)
        }
        Err(error) => {
            let status = error.status_code().as_u16();
            (failure(descriptor, error), status)
        }
    }
}
