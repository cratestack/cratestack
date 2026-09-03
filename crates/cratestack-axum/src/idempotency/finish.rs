//! What happens to the handler's response once it comes back: release,
//! forward, or buffer-and-persist.
//!
//! Split out of `service.rs` for the same reason `complete.rs`,
//! `reserve.rs` and `stream_bypass.rs` were before it — the workspace's
//! 200-line ceiling (`CLAUDE.md`), which ADR 0015 slice 1's `Option`
//! token pushed that file past. The three arms moved verbatim; the only
//! new one is the `None` token, which forwards a bypassed call's response
//! untouched.

use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_exec::OpExecutor;
use http::HeaderMap;

use crate::middleware_error::middleware_error_response;

use super::complete::buffer_and_persist_response;
use super::stream_bypass::{is_streamed_response, release_streamed_reservation};

/// `token` is `None` exactly when admission returned `Bypass`: no
/// reservation was taken, so there is nothing to complete or release and
/// the response is forwarded as-is — not even buffered, which is what
/// keeps a bypassed `@stream` response streaming.
///
/// Generic over the inner service's error type rather than naming
/// `Infallible`, because the release-on-error arm below exists precisely
/// for a future fallible inner service (see its comment).
pub(super) async fn finish_response<E>(
    executor: &OpExecutor,
    token: Option<uuid::Uuid>,
    principal: &str,
    key: &str,
    response_result: Result<Response, E>,
    error_headers: &HeaderMap,
    error_path: &str,
) -> Response {
    let response = match response_result {
        Ok(response) => response,
        Err(_) => {
            // `Service::Error = Infallible` so this branch is
            // unreachable in practice. The release-on-error path is
            // still here for if/when a fallible inner service is
            // plugged in. Guarding on `token` ensures a handler whose
            // reservation has already been reclaimed (TTL ran out)
            // doesn't drop the new owner's row.
            if let Some(token) = token {
                executor.release(principal, key, token).await;
            }
            return middleware_error_response(
                error_headers,
                error_path,
                CratestackError::Internal("handler returned an unrecoverable error".to_owned()),
            );
        }
    };
    if is_streamed_response(&response) {
        if let Some(token) = token {
            release_streamed_reservation(executor, principal, key, token).await;
        }
        return response;
    }
    let Some(token) = token else {
        return response;
    };
    buffer_and_persist_response(
        executor,
        principal,
        key,
        token,
        response,
        error_headers,
        error_path,
    )
    .await
}
