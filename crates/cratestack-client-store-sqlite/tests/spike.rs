use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Router, body::Bytes};
use cratestack_client_rust::{ClientConfig, ClientError, ClientStateStore, CratestackClient};
use cratestack_client_store_sqlite::SqliteStateStore;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CoolCodec, CoolErrorResponse};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FeedArgs {
    limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FeedItem {
    id: i64,
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CreatePostInput {
    title: String,
    author_id: i64,
}

#[derive(Clone)]
struct AppState {
    codec: CborCodec,
}

#[tokio::test]
async fn sqlite_store_journals_successful_procedure_call() {
    let (base_url, _server) = spawn_server().await;
    let store_path = project_tmp_path("procedure");
    cleanup_tmp_file(&store_path);
    let store = Arc::new(SqliteStateStore::open(&store_path).expect("store should open"));
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_state_store(store.clone());

    let output: Vec<FeedItem> = client
        .post(
            "/$procs/getFeed",
            &FeedArgs { limit: Some(4) },
            &[("x-auth-id", "9")],
        )
        .await
        .expect("procedure call should succeed");

    assert_eq!(output[0].id, 4);

    let state = store.load().expect("state should load");
    assert_eq!(state.state_version, 1);
    assert_eq!(state.request_journal.len(), 1);

    cleanup_tmp_file(&store_path);
}

#[tokio::test]
async fn sqlite_store_journals_generated_crud_error() {
    let (base_url, _server) = spawn_server().await;
    let store_path = project_tmp_path("crud");
    cleanup_tmp_file(&store_path);
    let store = Arc::new(SqliteStateStore::open(&store_path).expect("store should open"));
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_state_store(store.clone());

    let error = client
        .post::<_, FeedItem>(
            "/posts",
            &CreatePostInput {
                title: "Hello".to_owned(),
                author_id: 1,
            },
            &[],
        )
        .await
        .expect_err("anonymous create should be denied");

    match error {
        ClientError::Remote { status, error, .. } => {
            assert_eq!(status.as_u16(), 403);
            assert_eq!(error.expect("error body should decode").code, "FORBIDDEN");
        }
        other => panic!("expected remote error, got {other:?}"),
    }

    let state = store.load().expect("state should load");
    assert_eq!(state.state_version, 1);
    assert_eq!(state.request_journal.len(), 1);
    assert_eq!(state.request_journal[0].status_code, 403);

    cleanup_tmp_file(&store_path);
}

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed))
        .route("/posts", post(handle_create_post))
        .with_state(AppState { codec: CborCodec });
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
    (base_url, handle)
}

async fn handle_get_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: FeedArgs = state.codec.decode(&body).expect("request should decode");
    let payload = vec![FeedItem {
        id: args.limit.unwrap_or(1),
        title: "Feed".to_owned(),
    }];
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_create_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let _input: CreatePostInput = state.codec.decode(&body).expect("request should decode");
    let error = CoolErrorResponse {
        code: "FORBIDDEN".to_owned(),
        message: "forbidden: create requires authentication".to_owned(),
        details: None,
    };
    cbor_response(StatusCode::FORBIDDEN, &error)
}

fn codec_headers_ok(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    accept.contains(CborCodec::CONTENT_TYPE) && content_type == CborCodec::CONTENT_TYPE
}

fn cbor_response<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let body = CborCodec.encode(value).expect("response should encode");
    (
        status,
        [(axum::http::header::CONTENT_TYPE, CborCodec::CONTENT_TYPE)],
        body,
    )
        .into_response()
}

fn project_tmp_path(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/client-store-sqlite-tests")
        .join(format!("{label}-{suffix}.sqlite"))
}

fn cleanup_tmp_file(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).expect("tmp file should be removable");
    }
}
