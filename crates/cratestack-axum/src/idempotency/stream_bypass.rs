//! `IdempotencyService`'s response to cratestack#283: genuinely
//! incrementally-streamed responses (`@stream` procedures) must never
//! be silently re-buffered and replayed.
//!
//! `IdempotencyService::call` buffers the inner handler's response via
//! `axum::body::to_bytes` so it can hash + persist it for replay. That
//! is fine for ordinary responses (including today's buffered
//! `application/cbor-seq` sequence responses), but a genuinely
//! incremental one — built on `Body::from_stream`, potentially
//! long-lived or unbounded — buffering it defeats the entire point of
//! streaming and, for an unbounded stream, would never even finish
//! buffering.
//!
//! Two options were on the table (see cratestack#283's acceptance
//! criteria): reject the call outright, or bypass buffering and pass
//! the live stream straight through. Bypass is what's implemented,
//! because by the time this decision point is reached the handler has
//! **already run** — any side effects already happened. Rejecting at
//! this point wouldn't undo them; it would just throw away the
//! already-completed response and hand the caller a confusing error
//! instead of the data it asked for. So: idempotency *protection*
//! (dedup + replay) is skipped for this call — loudly, via the
//! `tracing::warn!` in `IdempotencyService::call`, not silently — but
//! the response itself is not discarded. The reservation this call took
//! is released so a legitimate retry with the same key isn't stuck
//! seeing "in flight" forever.
//!
//! This intentionally does **not** attempt a "streaming-aware
//! completion path" (recording a replay entry that itself replays
//! incrementally) — replaying a *partial* stream (client disconnected
//! mid-way through the original call) has no well-defined semantics,
//! and building one is explicitly out of this ticket's scope.

use axum::response::Response;

use crate::transport::StreamedResponseMarker;

/// True when `response` was produced by the genuinely incremental
/// `application/cbor-seq` encoder
/// (`crate::transport::stream_sequence::encode_cbor_sequence_stream_response`),
/// not the ordinary buffered one. Checked via `extensions()`, not a
/// header — see [`StreamedResponseMarker`]'s doc comment for why the
/// signal never touches `headers()` at all.
pub(super) fn is_streamed_response(response: &Response) -> bool {
    response
        .extensions()
        .get::<StreamedResponseMarker>()
        .is_some()
}

/// The bypass itself: the handler already ran, so refusing to forward its
/// output would only discard completed work. Instead release the
/// reservation — so a legitimate retry isn't stuck "in flight" forever —
/// and say so loudly.
///
/// Lives here rather than inline in `service.rs` so the rationale above
/// sits next to the marker it keys off (and to keep `service.rs` under
/// the workspace's 200-line ceiling). Moved verbatim in cratestack#846.
pub(super) async fn release_streamed_reservation(
    executor: &cratestack_exec::OpExecutor,
    principal: &str,
    key: &str,
    token: uuid::Uuid,
) {
    executor.release(principal, key, token).await;
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "idempotency",
        "idempotency key supplied for a @stream response body; streaming \
         responses are not idempotency-replayable — bypassing buffering/replay \
         for this call (see cratestack#283)",
    );
}

#[cfg(test)]
mod tests {
    use axum::body::Body;

    use super::*;

    #[test]
    fn detects_the_stream_marker_extension() {
        let mut response = Response::new(Body::empty());
        response.extensions_mut().insert(StreamedResponseMarker);
        assert!(is_streamed_response(&response));
    }

    #[test]
    fn ordinary_response_is_not_flagged() {
        let response = Response::new(Body::empty());
        assert!(!is_streamed_response(&response));
    }

    #[test]
    fn the_marker_never_touches_headers_so_it_cannot_leak_onto_the_wire() {
        // The whole point of using an extension instead of a header: even
        // if something upstream forgets to check is_streamed_response()
        // and forwards the response as-is, there is no header for a real
        // client to ever observe — extensions have no wire
        // representation at all, by construction of `http::Response`.
        let mut response = Response::new(Body::empty());
        response.extensions_mut().insert(StreamedResponseMarker);
        assert!(
            response.headers().is_empty(),
            "the stream marker must never be set as a header — it would leak to real clients"
        );
    }
}
