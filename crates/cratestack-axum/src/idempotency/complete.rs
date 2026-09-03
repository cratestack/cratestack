//! Buffers a live (non-streamed) handler response for idempotency
//! persistence and builds the final `Response` returned to the caller.
//! Split out of `service.rs` to keep that file under the repo's
//! ~200-LoC file ceiling — this is the tail of `IdempotencyService::call`
//! that runs once the response is known *not* to be a genuinely
//! incremental one (see `super::stream_bypass`, which short-circuits
//! before this ever runs for a `@stream` op's response).

use axum::body::Body;
use axum::response::Response;
use cratestack_core::CratestackError;
use http::HeaderMap;

use crate::middleware_error::middleware_error_response;

use super::headers::encode_headers;
use super::store::{IdempotencyStore, MAX_BODY_BYTES};

/// `request_headers`/`request_path` describe the *request*, not the
/// response being buffered: they exist only so the one error exit below
/// can negotiate its content type the same way every other error in the
/// stack does (cratestack#846).
pub(super) async fn buffer_and_persist_response(
    store: &dyn IdempotencyStore,
    principal: &str,
    key: &str,
    token: uuid::Uuid,
    response: Response,
    request_headers: &HeaderMap,
    request_path: &str,
) -> Response {
    let (rparts, rbody) = response.into_parts();
    let rbytes = match axum::body::to_bytes(rbody, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            // Drop the reservation so retries can attempt again — but
            // only if our token still holds.
            let _ = store.release(principal, key, token).await;
            // `Internal` already maps to 500, so no status override is
            // needed here — the pre-cratestack#846 code set it explicitly
            // because it built the response by hand.
            return middleware_error_response(
                request_headers,
                request_path,
                CratestackError::Internal("response body exceeded idempotency buffer".to_owned()),
            );
        }
    };
    // Capture the full header set so the replay reproduces the original
    // handler's `Location`, `ETag`, cache directives, `Content-Type`,
    // etc. Hop-by-hop and framework-computed headers are filtered
    // inside `encode_headers`. Pre-fix the middleware only persisted
    // `Content-Type`, so a `201 Created` with a `Location` header
    // replayed as `201 Created` with no `Location` — different
    // observable behaviour from the original execution.
    let headers_blob = encode_headers(&rparts.headers);

    // Persist the completion. Best-effort: on store failure we still
    // return the live response so the caller observes the mutation
    // that DID happen; banks running strict mode can wrap the store in
    // a fail-closed adapter. The `token` guard means a handler whose
    // reservation got reclaimed (TTL expired, retry took over) silently
    // fails this write rather than poisoning the newer reservation's
    // row.
    let _ = store
        .complete(
            principal,
            key,
            token,
            rparts.status.as_u16(),
            &headers_blob,
            &rbytes,
        )
        .await;
    Response::from_parts(rparts, Body::from(rbytes))
}
