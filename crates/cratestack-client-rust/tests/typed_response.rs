//! End-to-end proof (issue #493) that the `*_with_response` methods make
//! the `@version` optimistic-locking round trip reachable through the
//! typed client: `GET` → read `ETag` off `TypedResponse` → `PATCH` with
//! `If-Match` → `412` on a stale value, `200` + a bumped `ETag` on a
//! fresh one. The server here is a hand-rolled axum router standing in
//! for a real CrateStack server's `@version` handling (mirrors the
//! header shapes exercised end-to-end against Postgres in
//! `cratestack-pg`'s `tests/banking_versioning.rs::
//! http_patch_round_trips_etag_and_rejects_stale_if_match`), so this
//! test needs no database — it is scoped to proving the *client*
//! survives and forwards headers correctly, not re-proving server-side
//! locking.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Router, body::Bytes};
use cratestack_client_rust::{ClientConfig, CratestackClient, TypedResponse};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Ledger {
    id: i64,
    balance: i64,
    version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateLedgerInput {
    balance: i64,
}

#[derive(Clone)]
struct AppState {
    codec: CborCodec,
    // Single-row in-memory ledger, matching the row shape exercised by
    // `banking_versioning.cstack`.
    balance: Arc<AtomicI64>,
    version: Arc<AtomicI64>,
}

async fn handle_get(State(state): State<AppState>) -> Response {
    let ledger = Ledger {
        id: 4,
        balance: state.balance.load(Ordering::SeqCst),
        version: state.version.load(Ordering::SeqCst),
    };
    let etag = format!("\"{}\"", ledger.version);
    let body = state.codec.encode(&ledger).expect("value should encode");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, CborCodec::CONTENT_TYPE.to_owned()),
            (header::ETAG, etag),
        ],
        body,
    )
        .into_response()
}

async fn handle_patch(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let current_version = state.version.load(Ordering::SeqCst);
    let expected_etag = format!("\"{current_version}\"");
    let if_match = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok());

    if if_match != Some(expected_etag.as_str()) {
        let error = cratestack_core::CoolErrorResponse {
            code: "PRECONDITION_FAILED".to_owned(),
            message: "stale or missing If-Match".to_owned(),
            details: None,
        };
        let body = state.codec.encode(&error).expect("error should encode");
        return (
            StatusCode::PRECONDITION_FAILED,
            [(header::CONTENT_TYPE, CborCodec::CONTENT_TYPE)],
            body,
        )
            .into_response();
    }

    let input: UpdateLedgerInput = state.codec.decode(&body).expect("input should decode");
    state.balance.store(input.balance, Ordering::SeqCst);
    let new_version = state.version.fetch_add(1, Ordering::SeqCst) + 1;

    let ledger = Ledger {
        id: 4,
        balance: input.balance,
        version: new_version,
    };
    let new_etag = format!("\"{new_version}\"");
    let body = state.codec.encode(&ledger).expect("value should encode");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, CborCodec::CONTENT_TYPE.to_owned()),
            (header::ETAG, new_etag),
        ],
        body,
    )
        .into_response()
}

// A real CrateStack server now enforces `If-Match` on `DELETE` for an
// `@version` model exactly like `PATCH` (cratestack#519); this
// hand-rolled mock deliberately does *not* reproduce that check —
// it exists only to prove `delete_with_response` plumbs status and
// headers through the client, which `handle_patch` above already
// covers for the `If-Match`-checking case. The real server-side
// enforcement is proven end-to-end against Postgres by
// `cratestack-pg`'s `tests/banking_versioning.rs`. Returns the
// pre-delete record plus a custom header, standing in for the kind
// of out-of-band signal (a `Retry-After`, an audit marker, etc.)
// `delete_with_response` exists to surface.
async fn handle_delete(State(state): State<AppState>) -> Response {
    let ledger = Ledger {
        id: 4,
        balance: state.balance.load(Ordering::SeqCst),
        version: state.version.load(Ordering::SeqCst),
    };
    let body = state.codec.encode(&ledger).expect("value should encode");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, CborCodec::CONTENT_TYPE.to_owned()),
            (
                header::HeaderName::from_static("x-deleted-by"),
                "test-suite".to_owned(),
            ),
        ],
        body,
    )
        .into_response()
}

async fn spawn_server() -> (Url, AppState, tokio::task::JoinHandle<()>) {
    let state = AppState {
        codec: CborCodec,
        balance: Arc::new(AtomicI64::new(1)),
        version: Arc::new(AtomicI64::new(0)),
    };
    let app = Router::new()
        .route(
            "/ledgers/4",
            get(handle_get).patch(handle_patch).delete(handle_delete),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL should parse");
    (base_url, state, handle)
}

#[tokio::test]
async fn get_with_response_then_patch_with_response_round_trips_etag_and_if_match() {
    let (base_url, _state, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    // GET must expose the ETag through TypedResponse — the plain `get`
    // signature has no way to do this, which is the whole reason #493
    // exists.
    let get_response: TypedResponse<Ledger> = client
        .get_with_response("/ledgers/4", &[], &[])
        .await
        .expect("get_with_response should succeed");
    assert_eq!(get_response.status, StatusCode::OK);
    assert_eq!(get_response.value.version, 0);
    let etag = get_response
        .header("etag")
        .expect("etag header must survive decoding")
        .to_owned();
    assert_eq!(etag, "\"0\"");

    // PATCH with the learned ETag as If-Match must succeed and return a
    // bumped ETag in the response headers.
    let patch_response: TypedResponse<Ledger> = client
        .patch_with_response(
            "/ledgers/4",
            &UpdateLedgerInput { balance: 99 },
            &[("if-match", etag.as_str())],
        )
        .await
        .expect("patch_with_response with fresh If-Match should succeed");
    assert_eq!(patch_response.status, StatusCode::OK);
    assert_eq!(patch_response.value.balance, 99);
    assert_eq!(patch_response.value.version, 1);
    assert_eq!(patch_response.header("etag"), Some("\"1\""));
}

#[tokio::test]
async fn patch_with_response_surfaces_412_for_a_stale_if_match() {
    let (base_url, _state, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let error = client
        .patch_with_response::<_, Ledger>(
            "/ledgers/4",
            &UpdateLedgerInput { balance: 5 },
            &[("if-match", "\"999\"")],
        )
        .await
        .expect_err("stale If-Match must be rejected");

    match error {
        cratestack_client_rust::ClientError::Remote { status, error, .. } => {
            assert_eq!(status.as_u16(), 412);
            assert_eq!(
                error.expect("error body should decode").code,
                "PRECONDITION_FAILED"
            );
        }
        other => panic!("expected a remote 412 error, got {other:?}"),
    }
}

/// Proves `delete_with_response` surfaces status and headers like its
/// siblings. Unlike the `get_with_response` → `patch_with_response` round
/// trip above, this sends **no** `If-Match` against a mock `handle_delete`
/// that (unlike a real CrateStack server as of cratestack#519) doesn't
/// check for one — so it only proves the response metadata plumbing works
/// on this verb too, not any concurrency-safety semantics. The real
/// server-side `If-Match` enforcement on `DELETE` is proven end-to-end
/// against Postgres by `cratestack-pg`'s `tests/banking_versioning.rs`.
#[tokio::test]
async fn delete_with_response_surfaces_status_and_headers() {
    let (base_url, _state, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let response: TypedResponse<Ledger> = client
        .delete_with_response("/ledgers/4", &[])
        .await
        .expect("delete_with_response should succeed");

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.value.id, 4);
    assert_eq!(response.header("x-deleted-by"), Some("test-suite"));
}

/// Proves the original `get`/`patch` signatures are unaffected by #493:
/// they still return the bare decoded value, with no headers attached,
/// and this crate's own body-shape assertion is enough to prove the
/// caller never needs to change anything to keep using them exactly as
/// before.
#[tokio::test]
async fn plain_get_and_patch_are_unchanged_and_still_only_return_the_value() {
    let (base_url, _state, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let ledger: Ledger = client
        .get("/ledgers/4", &[], &[])
        .await
        .expect("plain get should still succeed");
    assert_eq!(ledger.version, 0);

    let updated: Ledger = client
        .patch(
            "/ledgers/4",
            &UpdateLedgerInput { balance: 7 },
            &[("if-match", "\"0\"")],
        )
        .await
        .expect("plain patch should still succeed with a correct If-Match");
    assert_eq!(updated.balance, 7);
    assert_eq!(updated.version, 1);
}
