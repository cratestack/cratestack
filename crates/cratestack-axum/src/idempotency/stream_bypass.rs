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

use crate::transport::STREAM_RESPONSE_HEADER;

/// True when `response` was produced by the genuinely incremental
/// `application/cbor-seq` encoder
/// (`crate::transport::stream_sequence::encode_cbor_sequence_stream_response`),
/// not the ordinary buffered one.
pub(super) fn is_streamed_response(response: &Response) -> bool {
    response.headers().contains_key(STREAM_RESPONSE_HEADER)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn detects_the_stream_marker_header() {
        let mut response = Response::new(Body::empty());
        response.headers_mut().insert(
            STREAM_RESPONSE_HEADER,
            HeaderValue::from_static("incremental"),
        );
        assert!(is_streamed_response(&response));
    }

    #[test]
    fn ordinary_response_is_not_flagged() {
        let response = Response::new(Body::empty());
        assert!(!is_streamed_response(&response));
    }
}
