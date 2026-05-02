use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, body::Bytes};
use cratestack_client_rust::{
    AuthorizationRequest, ClientConfig, ClientError, ClientStateStore, CratestackClient, JsonCodec,
    JsonFileStateStore, RequestAuthorizer, RuntimeCodecConfig, RuntimeConfigWire,
    RuntimeEnvelopeConfig, RuntimeHandle, RuntimeRequestWire, RuntimeStateStoreConfig,
    RuntimeTransportConfig,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CoolCodec, CoolErrorResponse, SelectionQuery, canonical_request_string};
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

struct CanonicalAuthorizationAuthorizer;

impl RequestAuthorizer for CanonicalAuthorizationAuthorizer {
    fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        assert_eq!(
            request.content_type.as_deref(),
            Some(CborCodec::CONTENT_TYPE)
        );
        assert!(!request.body.is_empty());
        Ok(vec![(
            "authorization".to_owned(),
            format!(
                "Signature {}",
                hex_string(request.canonical_request.as_bytes())
            ),
        )])
    }
}

#[tokio::test]
async fn cbor_client_spike_calls_procedure_and_journals_request() {
    let (base_url, _server) = spawn_server().await;
    let store_path = project_tmp_path("procedure-spike.json");
    cleanup_tmp_file(&store_path);
    let store = Arc::new(JsonFileStateStore::new(&store_path));
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_state_store(store.clone());

    let output: Vec<FeedItem> = client
        .post(
            "/$procs/getFeed",
            &FeedArgs { limit: Some(7) },
            &[("x-auth-id", "42")],
        )
        .await
        .expect("procedure call should succeed");

    assert_eq!(output.len(), 1);
    assert_eq!(output[0].id, 7);

    let state = store.load().expect("state should load");
    assert_eq!(state.request_journal.len(), 1);
    assert_eq!(state.request_journal[0].path, "/$procs/getFeed");
    assert_eq!(state.request_journal[0].status_code, 200);

    cleanup_tmp_file(&store_path);
}

#[tokio::test]
async fn cbor_client_spike_decodes_crud_error_and_journals_request() {
    let (base_url, _server) = spawn_server().await;
    let store_path = project_tmp_path("crud-spike.json");
    cleanup_tmp_file(&store_path);
    let store = Arc::new(JsonFileStateStore::new(&store_path));
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_state_store(store.clone());

    let error = client
        .post::<_, FeedItem>(
            "/posts",
            &CreatePostInput {
                title: "Hello".to_owned(),
                author_id: 7,
            },
            &[],
        )
        .await
        .expect_err("anonymous create should be denied before database access");

    match error {
        ClientError::Remote { status, error, .. } => {
            assert_eq!(status.as_u16(), 403);
            assert_eq!(error.expect("error body should decode").code, "FORBIDDEN");
        }
        other => panic!("expected remote error, got {other:?}"),
    }

    let state = store.load().expect("state should load");
    assert_eq!(state.request_journal.len(), 1);
    assert_eq!(state.request_journal[0].path, "/posts");
    assert_eq!(state.request_journal[0].status_code, 403);

    cleanup_tmp_file(&store_path);
}

#[tokio::test]
async fn cbor_client_spike_fetches_projected_record() {
    let (base_url, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let mut include_fields = BTreeMap::new();
    include_fields.insert("author".to_owned(), vec!["email".to_owned()]);
    let selection = SelectionQuery {
        fields: vec!["id".to_owned(), "title".to_owned()],
        includes: vec!["author".to_owned()],
        include_fields,
    };

    let value = client
        .get_view("/posts/1", &selection, &[])
        .await
        .expect("projected fetch should succeed");

    assert_eq!(value["id"], serde_json::json!(1));
    assert_eq!(value["title"], serde_json::json!("Published Post"));
    assert_eq!(value["subtitle"], serde_json::Value::Null);
    assert_eq!(
        value["author"]["email"],
        serde_json::json!("owner@example.com")
    );
}

#[tokio::test]
async fn cbor_client_spike_lists_projected_records() {
    let (base_url, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let selection = SelectionQuery {
        fields: vec!["id".to_owned(), "title".to_owned()],
        includes: Vec::new(),
        include_fields: BTreeMap::new(),
    };

    let value = client
        .list_view("/posts", &selection, &[("limit", "2")], &[])
        .await
        .expect("projected list should succeed");

    assert_eq!(value.len(), 2);
    assert_eq!(value[0]["id"], serde_json::json!(1));
    assert_eq!(value[1]["title"], serde_json::json!("Second Post"));
}

#[tokio::test]
async fn cbor_client_spike_rejects_duplicate_projection_keys_in_extra_query() {
    let client = CratestackClient::new(
        ClientConfig::new(Url::parse("http://127.0.0.1:1/").expect("url should parse")),
        CborCodec,
    );
    let selection = SelectionQuery {
        fields: vec!["id".to_owned()],
        includes: Vec::new(),
        include_fields: BTreeMap::new(),
    };

    let error = client
        .list_view("/posts", &selection, &[("fields", "title")], &[])
        .await
        .expect_err("duplicate projection keys should fail before transport");

    match error {
        ClientError::BadInput(message) => {
            assert!(message.contains("SelectionQuery"));
        }
        other => panic!("expected bad input error, got {other:?}"),
    }
}

#[tokio::test]
async fn cbor_client_spike_authorizes_requests_from_canonical_input() {
    let (base_url, _server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_request_authorizer(Arc::new(CanonicalAuthorizationAuthorizer));

    let output: Vec<FeedItem> = client
        .post(
            "/$procs/getFeedSigned",
            &FeedArgs { limit: Some(3) },
            &[("x-auth-id", "42")],
        )
        .await
        .expect("authorized procedure call should succeed");

    assert_eq!(output[0].id, 3);
}

#[test]
fn runtime_handle_json_transport_round_trips_bridge_json_bytes() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_json_server());
    let handle = RuntimeHandle::new(RuntimeConfigWire {
        base_url: base_url.to_string(),
        state_store: RuntimeStateStoreConfig::InMemory,
        transport: RuntimeTransportConfig {
            codec: RuntimeCodecConfig::Json,
            envelope: RuntimeEnvelopeConfig::None,
        },
    })
    .expect("runtime handle should build");

    let response = handle
        .execute(RuntimeRequestWire {
            method: "POST".to_owned(),
            path: "/$procs/getFeed".to_owned(),
            canonical_query: None,
            headers: Vec::new(),
            body: serde_json::to_vec(&FeedArgs { limit: Some(5) })
                .expect("json bridge payload should encode"),
        })
        .expect("runtime request should succeed");

    let payload: Vec<FeedItem> =
        serde_json::from_slice(&response.body).expect("bridge response should decode");
    assert_eq!(payload[0].id, 5);
}

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed))
        .route("/$procs/getFeedSigned", post(handle_signed_get_feed))
        .route("/posts", get(handle_list_posts).post(handle_create_post))
        .route("/posts/1", get(handle_get_post))
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

async fn spawn_json_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/$procs/getFeed", post(handle_get_feed_json))
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

    let _auth_id = headers
        .get("x-auth-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let args: FeedArgs = state.codec.decode(&body).expect("request should decode");
    let payload = vec![FeedItem {
        id: args.limit.unwrap_or(1),
        title: "Feed".to_owned(),
    }];
    cbor_response(StatusCode::OK, &payload)
}

async fn handle_signed_get_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !codec_headers_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let expected = format!(
        "Signature {}",
        hex_string(
            canonical_request_string(
                "POST",
                "/$procs/getFeedSigned",
                None,
                Some(CborCodec::CONTENT_TYPE),
                &body,
            )
            .as_bytes(),
        )
    );
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if authorization != expected {
        return (StatusCode::UNAUTHORIZED, Vec::<u8>::new()).into_response();
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

async fn handle_get_post(uri: Uri, headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }
    if uri.query() != Some("fields=id%2Ctitle&include=author&includeFields%5Bauthor%5D=email") {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let payload = serde_json::json!({
        "id": 1,
        "title": "Published Post",
        "subtitle": null,
        "author": {
            "email": "owner@example.com"
        }
    });
    projected_response(&headers, StatusCode::OK, &payload)
}

async fn handle_list_posts(uri: Uri, headers: HeaderMap) -> Response {
    if !accept_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }
    if uri.query() != Some("fields=id%2Ctitle&limit=2") {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let payload = serde_json::json!([
        {
            "id": 1,
            "title": "Published Post",
            "subtitle": null
        },
        {
            "id": 2,
            "title": "Second Post",
            "subtitle": null
        }
    ]);
    projected_response(&headers, StatusCode::OK, &payload)
}

async fn handle_get_feed_json(headers: HeaderMap, body: Bytes) -> Response {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !accept.contains("application/json") || content_type != "application/json" {
        return (StatusCode::BAD_REQUEST, Vec::<u8>::new()).into_response();
    }

    let args: FeedArgs = serde_json::from_slice(&body).expect("request should decode");
    let payload = vec![FeedItem {
        id: args.limit.unwrap_or(1),
        title: "Feed".to_owned(),
    }];
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(&payload).expect("response should encode"),
    )
        .into_response()
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

fn accept_header_ok(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .contains(JsonCodec::CONTENT_TYPE)
}

fn projected_response<T: serde::Serialize>(
    headers: &HeaderMap,
    status: StatusCode,
    value: &T,
) -> Response {
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if accept.contains(JsonCodec::CONTENT_TYPE) {
        json_response(status, value)
    } else {
        cbor_response(status, value)
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let body = JsonCodec.encode(value).expect("response should encode");
    (
        status,
        [(axum::http::header::CONTENT_TYPE, JsonCodec::CONTENT_TYPE)],
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
        .join("../../tmp/client-rust-tests")
        .join(format!("{label}-{suffix}.json"))
}

fn cleanup_tmp_file(path: &PathBuf) {
    if path.exists() {
        std::fs::remove_file(path).expect("tmp file should be removable");
    }
}
