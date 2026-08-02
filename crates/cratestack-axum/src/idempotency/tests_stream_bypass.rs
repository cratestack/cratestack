//! Integration-shaped regression test for cratestack#283: a genuinely
//! streamed response (marked via `STREAM_RESPONSE_HEADER`, exactly as
//! `crate::transport::stream_sequence` sets it) must pass through
//! `IdempotencyService` untouched, with its reservation released rather
//! than left dangling — not silently re-buffered into a replay record.
//! Drives the real `Service` impl (not just the `is_streamed_response`
//! predicate unit-tested in `stream_bypass`), with an in-memory
//! `IdempotencyStore` double standing in for the sqlx/redis
//! implementations this crate doesn't depend on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::HeaderValue;
use axum::response::Response;
use cratestack_core::CoolError;
use futures_util::stream;
use http::StatusCode;
use tower::{Layer, Service};

use super::layer::IdempotencyLayer;
use super::record::{IdempotencyRecord, ReservationOutcome};
use super::store::IdempotencyStore;
use crate::transport::STREAM_RESPONSE_HEADER;

#[derive(Default)]
struct InMemoryIdempotencyStore {
    entries: Mutex<HashMap<(String, String), Entry>>,
}

struct Entry {
    token: uuid::Uuid,
    hash: [u8; 32],
    record: Option<IdempotencyRecord>,
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        match entries.get(&map_key) {
            None => {
                let token = uuid::Uuid::new_v4();
                entries.insert(
                    map_key,
                    Entry {
                        token,
                        hash: request_hash,
                        record: None,
                    },
                );
                Ok(ReservationOutcome::Reserved { token })
            }
            Some(entry) if entry.hash != request_hash => Ok(ReservationOutcome::Conflict),
            Some(entry) => Ok(match &entry.record {
                Some(record) => ReservationOutcome::Replay(record.clone()),
                None => ReservationOutcome::InFlight,
            }),
        }
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CoolError> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(&(principal.to_owned(), key.to_owned()))
            && entry.token == token {
                entry.record = Some(IdempotencyRecord {
                    key: key.to_owned(),
                    principal_fingerprint: principal.to_owned(),
                    request_hash: entry.hash,
                    response_status: status,
                    response_headers: headers.to_vec(),
                    response_body: body.to_vec(),
                    created_at: SystemTime::now(),
                    expires_at: SystemTime::now(),
                });
            }
        Ok(())
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
    ) -> Result<(), CoolError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        if entries
            .get(&map_key)
            .is_some_and(|entry| entry.token == token)
        {
            entries.remove(&map_key);
        }
        Ok(())
    }
}

fn streamed_request() -> Request {
    Request::builder()
        .method("POST")
        .uri("/rpc/procedure.ticks")
        .header("idempotency-key", "same-key-both-times")
        .body(Body::empty())
        .unwrap()
}

fn streamed_response() -> Response {
    let body = Body::from_stream(stream::iter(vec![Ok::<_, std::convert::Infallible>(
        Bytes::from_static(b"chunk"),
    )]));
    let mut response = Response::new(body);
    response.headers_mut().insert(
        STREAM_RESPONSE_HEADER,
        HeaderValue::from_static("incremental"),
    );
    response
}

#[tokio::test]
async fn stream_response_bypasses_buffering_and_releases_its_reservation() {
    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let call_count = Arc::new(AtomicUsize::new(0));
    let inner_calls = call_count.clone();
    let inner = tower::service_fn(move |_req: Request| {
        let call_count = inner_calls.clone();
        async move {
            call_count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(streamed_response())
        }
    });
    let mut svc = IdempotencyLayer::new(store, Duration::from_secs(60)).layer(inner);

    let first = svc.call(streamed_request()).await.unwrap();
    assert_eq!(
        first
            .headers()
            .get(STREAM_RESPONSE_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some("incremental"),
        "the live stream must pass through untouched, marker header included"
    );
    let first_body = axum::body::to_bytes(first.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&first_body[..], b"chunk");

    // The reservation the first call took must have been released — not
    // left dangling (which would make every retry with this key see
    // `InFlight`/409 forever) and not completed into a replay record
    // (which would silently defeat the entire point of streaming on
    // the next call). A second call with the identical key must
    // therefore re-run the handler rather than block or replay.
    let second = svc.call(streamed_request()).await.unwrap();
    assert_eq!(
        second.status(),
        StatusCode::OK,
        "must not be rejected as `InFlight`/409 — the reservation was released"
    );
    let second_body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&second_body[..], b"chunk");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "handler must run again on the second call — a streamed response is never replayed"
    );
}
